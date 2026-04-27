use std::num::NonZeroU32;

use imap_client::client::tokio::Client as ImapClient;
use imap_client::imap_types::{
    command::CommandBody,
    fetch::MessageDataItem,
    fetch::{MacroOrMessageDataItemNames, MessageDataItemName},
    flag::{Flag, StoreType},
    response::{Data, StatusBody, StatusKind},
    sequence::{SeqOrUid, SequenceSet},
    IntoStatic,
};
use imap_client::tasks::tasks::TaskError;
use imap_client::tasks::Task;
use posthaste_domain::{ImapMessageLocation, MailboxId, MutationOutcome, SetKeywordsCommand};

use crate::discovery::connect_authenticated_client;
use crate::{selected_mailbox_from_examine, ImapAdapterError, ImapConnectionConfig};

/// Apply a JMAP keyword delta using UID STORE in the selected IMAP mailbox.
///
/// The command validates the stored UIDVALIDITY epoch before issuing STORE so a
/// stale UID cannot mutate a different message after provider-side UID reuse.
/// Keyword mutations use `.SILENT` because Posthaste already knows the intended
/// delta and real providers may omit or sparsely populate the untagged FETCH
/// response for accepted STORE commands.
///
/// @spec docs/L1-api#message-commands
pub async fn apply_imap_keyword_delta_by_location(
    config: &ImapConnectionConfig,
    mailbox_name: &str,
    location: &ImapMessageLocation,
    command: &SetKeywordsCommand,
) -> Result<MutationOutcome, ImapAdapterError> {
    let mut client = connect_authenticated_client(config).await?;
    select_validated_mailbox(&mut client, mailbox_name, location).await?;

    let uid_set = uid_sequence_set(location)?;
    let add_flags = imap_flags_for_keywords(&command.add)?;
    let remove_flags = imap_flags_for_keywords(&command.remove)?;
    if !add_flags.is_empty() {
        client
            .uid_silent_store(uid_set.clone(), StoreType::Add, add_flags)
            .await
            .map_err(ImapAdapterError::from)?;
    }
    if !remove_flags.is_empty() {
        client
            .uid_silent_store(uid_set, StoreType::Remove, remove_flags)
            .await
            .map_err(ImapAdapterError::from)?;
    }

    Ok(MutationOutcome { cursor: None })
}

/// Copy one IMAP message to another mailbox.
///
/// `imap-client` currently exposes COPY success but not COPYUID output, so this
/// command validates the source UID before COPY and relies on the next sync to
/// discover the destination UID location.
///
/// @spec docs/L1-api#message-commands
pub async fn copy_imap_message_to_mailbox_by_location(
    config: &ImapConnectionConfig,
    source_mailbox_name: &str,
    location: &ImapMessageLocation,
    target_mailbox_name: &str,
) -> Result<(), ImapAdapterError> {
    let mut client = connect_authenticated_client(config).await?;
    select_validated_mailbox(&mut client, source_mailbox_name, location).await?;
    verify_uid_fetch_response(&mut client, location).await?;
    client
        .uid_copy(uid_sequence_set(location)?, target_mailbox_name)
        .await
        .map_err(ImapAdapterError::from)
}

/// Move one IMAP message to another mailbox with UID MOVE.
///
/// `imap-client` exposes MOVE success but not COPYUID output. Even when the
/// server supports UIDPLUS, the adapter validates the source UID before MOVE
/// and relies on the next sync to discover the destination UID location.
///
/// @spec docs/L1-api#message-commands
pub async fn move_imap_message_to_mailbox_by_location(
    config: &ImapConnectionConfig,
    source_mailbox_name: &str,
    location: &ImapMessageLocation,
    target_mailbox_name: &str,
) -> Result<(), ImapAdapterError> {
    let mut client = connect_authenticated_client(config).await?;
    select_validated_mailbox(&mut client, source_mailbox_name, location).await?;
    verify_uid_fetch_response(&mut client, location).await?;
    client
        .uid_move(uid_sequence_set(location)?, target_mailbox_name)
        .await
        .map_err(ImapAdapterError::from)
}

/// Mark one IMAP message as `\Deleted` without issuing broad EXPUNGE.
///
/// This avoids the RFC 6851/RFC 4315 footgun where plain EXPUNGE can remove
/// other clients' deleted messages. A later UID EXPUNGE wrapper can make this
/// a true permanent delete when the dependency exposes it.
///
/// @spec docs/L1-api#message-commands
pub async fn mark_imap_message_deleted_by_location(
    config: &ImapConnectionConfig,
    mailbox_name: &str,
    location: &ImapMessageLocation,
) -> Result<MutationOutcome, ImapAdapterError> {
    let mut client = connect_authenticated_client(config).await?;
    select_validated_mailbox(&mut client, mailbox_name, location).await?;
    verify_uid_fetch_response(&mut client, location).await?;
    client
        .uid_silent_store(uid_sequence_set(location)?, StoreType::Add, [Flag::Deleted])
        .await
        .map_err(ImapAdapterError::from)?;

    Ok(MutationOutcome { cursor: None })
}

/// Mark and permanently expunge one IMAP message using UID EXPUNGE.
///
/// Only call this when the server advertises UIDPLUS or IMAP4rev2 support.
///
/// @spec docs/L1-api#message-commands
pub async fn expunge_imap_message_by_location(
    config: &ImapConnectionConfig,
    mailbox_name: &str,
    location: &ImapMessageLocation,
) -> Result<MutationOutcome, ImapAdapterError> {
    let mut client = connect_authenticated_client(config).await?;
    select_validated_mailbox(&mut client, mailbox_name, location).await?;
    verify_uid_fetch_response(&mut client, location).await?;
    client
        .uid_silent_store(uid_sequence_set(location)?, StoreType::Add, [Flag::Deleted])
        .await
        .map_err(ImapAdapterError::from)?;
    let _expunged = uid_expunge(&mut client, location).await?;

    Ok(MutationOutcome { cursor: None })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImapMailboxReplacementDelta {
    pub add: Vec<MailboxId>,
    pub remove: Vec<MailboxId>,
}

pub fn imap_mailbox_replacement_delta(
    current_mailbox_ids: &[MailboxId],
    target_mailbox_ids: &[MailboxId],
) -> ImapMailboxReplacementDelta {
    let current = current_mailbox_ids
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let target = target_mailbox_ids
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();

    ImapMailboxReplacementDelta {
        add: target.difference(&current).cloned().collect(),
        remove: current.difference(&target).cloned().collect(),
    }
}

pub fn imap_flags_for_keywords(
    keywords: &[String],
) -> Result<Vec<Flag<'static>>, ImapAdapterError> {
    keywords
        .iter()
        .map(|keyword| imap_flag_for_keyword(keyword))
        .collect()
}

fn imap_flag_for_keyword(keyword: &str) -> Result<Flag<'static>, ImapAdapterError> {
    let flag = match keyword.to_ascii_lowercase().as_str() {
        "$seen" => Flag::Seen,
        "$flagged" => Flag::Flagged,
        "$answered" => Flag::Answered,
        "$draft" => Flag::Draft,
        "$forwarded" => Flag::try_from("\\Forwarded").expect("static IMAP flag is valid"),
        _ => Flag::try_from(keyword)
            .map_err(|error| ImapAdapterError::InvalidKeywordFlag {
                keyword: keyword.to_string(),
                reason: error.to_string(),
            })?
            .into_static(),
    };

    Ok(flag)
}

fn uid_sequence_set(location: &ImapMessageLocation) -> Result<SequenceSet, ImapAdapterError> {
    let uid = NonZeroU32::new(location.uid.0)
        .ok_or_else(|| ImapAdapterError::InvalidUidSequence("UID 0".to_string()))?;
    Ok(SequenceSet::from(SeqOrUid::from(uid)))
}

async fn select_validated_mailbox(
    client: &mut ImapClient,
    mailbox_name: &str,
    location: &ImapMessageLocation,
) -> Result<(), ImapAdapterError> {
    let selected = selected_mailbox_from_examine(mailbox_name, client.select(mailbox_name).await?)?;
    if selected.uid_validity != location.uid_validity {
        return Err(ImapAdapterError::UidValidityMismatch {
            mailbox_name: mailbox_name.to_string(),
            expected: location.uid_validity.0,
            actual: selected.uid_validity.0,
        });
    }
    Ok(())
}

async fn verify_uid_fetch_response(
    client: &mut ImapClient,
    location: &ImapMessageLocation,
) -> Result<(), ImapAdapterError> {
    let items = client
        .uid_fetch_first(uid(location)?, uid_fetch_item_names())
        .await
        .map_err(ImapAdapterError::from)?;
    verify_message_data_contains_uid(location, items, "matching UID FETCH response")
}

fn uid(location: &ImapMessageLocation) -> Result<NonZeroU32, ImapAdapterError> {
    NonZeroU32::new(location.uid.0)
        .ok_or_else(|| ImapAdapterError::InvalidUidSequence("UID 0".to_string()))
}

fn uid_fetch_item_names() -> MacroOrMessageDataItemNames<'static> {
    MacroOrMessageDataItemNames::MessageDataItemNames(vec![MessageDataItemName::Uid])
}

fn verify_message_data_contains_uid(
    location: &ImapMessageLocation,
    items: impl IntoIterator<Item = MessageDataItem<'static>>,
    missing_label: &'static str,
) -> Result<(), ImapAdapterError> {
    let found_matching_uid = items.into_iter().any(|item| match item {
        MessageDataItem::Uid(uid) => uid.get() == location.uid.0,
        _ => false,
    });
    if found_matching_uid {
        Ok(())
    } else {
        Err(ImapAdapterError::MissingFetchData(missing_label))
    }
}

async fn uid_expunge(
    client: &mut ImapClient,
    location: &ImapMessageLocation,
) -> Result<Vec<NonZeroU32>, ImapAdapterError> {
    client
        .resolve(UidExpungeTask::new(uid_sequence_set(location)?))
        .await
        .map_err(ImapAdapterError::from)?
        .map_err(|error| ImapAdapterError::Client(error.to_string()))
}

#[derive(Clone, Debug)]
struct UidExpungeTask {
    sequence_set: SequenceSet,
    output: Vec<NonZeroU32>,
}

impl UidExpungeTask {
    fn new(sequence_set: SequenceSet) -> Self {
        Self {
            sequence_set,
            output: Vec::new(),
        }
    }
}

impl Task for UidExpungeTask {
    type Output = Result<Vec<NonZeroU32>, TaskError>;

    fn command_body(&self) -> CommandBody<'static> {
        CommandBody::ExpungeUid {
            sequence_set: self.sequence_set.clone(),
        }
    }

    fn process_data(&mut self, data: Data<'static>) -> Option<Data<'static>> {
        if let Data::Expunge(seq) = data {
            self.output.push(seq);
            None
        } else {
            Some(data)
        }
    }

    fn process_tagged(self, status_body: StatusBody<'static>) -> Self::Output {
        match status_body.kind {
            StatusKind::Ok => Ok(self.output),
            StatusKind::No => Err(TaskError::UnexpectedNoResponse(status_body)),
            StatusKind::Bad => Err(TaskError::UnexpectedBadResponse(status_body)),
        }
    }
}

#[cfg(test)]
mod tests {
    use imap_client::imap_types::flag::Flag;
    use posthaste_domain::{ImapUid, ImapUidValidity, MailboxId, MessageId};

    use super::*;

    #[test]
    fn maps_jmap_keywords_to_imap_system_flags() {
        let flags = imap_flags_for_keywords(&[
            "$seen".to_string(),
            "$flagged".to_string(),
            "$answered".to_string(),
            "$draft".to_string(),
            "$forwarded".to_string(),
        ])
        .expect("flags");

        assert_eq!(
            flags,
            vec![
                Flag::Seen,
                Flag::Flagged,
                Flag::Answered,
                Flag::Draft,
                Flag::try_from("\\Forwarded").expect("forwarded flag"),
            ]
        );
    }

    #[test]
    fn preserves_custom_keywords_as_imap_keywords() {
        let flags =
            imap_flags_for_keywords(&["project-x".to_string()]).expect("custom keyword flag");

        assert_eq!(
            flags,
            vec![Flag::try_from("project-x").expect("custom keyword")]
        );
    }

    #[test]
    fn rejects_keywords_that_are_not_valid_imap_atoms() {
        let error = imap_flags_for_keywords(&["bad keyword".to_string()])
            .expect_err("spaces are not valid atom characters");

        assert!(matches!(
            error,
            ImapAdapterError::InvalidKeywordFlag {
                keyword,
                ..
            } if keyword == "bad keyword"
        ));
    }

    #[test]
    fn computes_mailbox_replacement_delta() {
        let delta = imap_mailbox_replacement_delta(
            &[
                MailboxId::from("imap:mailbox:inbox"),
                MailboxId::from("imap:mailbox:archive"),
            ],
            &[
                MailboxId::from("imap:mailbox:archive"),
                MailboxId::from("imap:mailbox:trash"),
            ],
        );

        assert_eq!(
            delta,
            ImapMailboxReplacementDelta {
                add: vec![MailboxId::from("imap:mailbox:trash")],
                remove: vec![MailboxId::from("imap:mailbox:inbox")],
            }
        );
    }

    #[test]
    fn rejects_uid_fetch_response_without_matching_uid() {
        let error = verify_message_data_contains_uid(
            &location(),
            [MessageDataItem::Uid(NonZeroU32::new(99).expect("uid"))],
            "matching UID FETCH response",
        )
        .expect_err("matching UID is required");

        assert!(matches!(
            error,
            ImapAdapterError::MissingFetchData("matching UID FETCH response")
        ));
    }

    #[test]
    fn uid_expunge_task_uses_uid_expunge_command_body() {
        let task = UidExpungeTask::new(uid_sequence_set(&location()).expect("uid set"));

        let CommandBody::ExpungeUid { .. } = task.command_body() else {
            panic!("UID EXPUNGE command body is required");
        };
    }

    fn location() -> ImapMessageLocation {
        ImapMessageLocation {
            message_id: MessageId::from("message-1"),
            mailbox_id: MailboxId::from("imap:mailbox:494e424f58"),
            uid_validity: ImapUidValidity(9),
            uid: ImapUid(42),
            modseq: None,
            updated_at: "2026-04-25T00:00:00Z".to_string(),
        }
    }
}
