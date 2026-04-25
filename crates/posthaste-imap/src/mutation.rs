use std::num::NonZeroU32;

use imap_client::imap_types::{
    fetch::MessageDataItem,
    flag::{Flag, StoreType},
    sequence::{SeqOrUid, SequenceSet},
    IntoStatic,
};
use posthaste_domain::{ImapMessageLocation, MutationOutcome, SetKeywordsCommand};

use crate::discovery::connect_authenticated_client;
use crate::{selected_mailbox_from_examine, ImapAdapterError, ImapConnectionConfig};

/// Apply a JMAP keyword delta using UID STORE in the selected IMAP mailbox.
///
/// The command validates the stored UIDVALIDITY epoch before issuing STORE so a
/// stale UID cannot mutate a different message after provider-side UID reuse.
///
/// @spec docs/L1-api#message-commands
pub async fn apply_imap_keyword_delta_by_location(
    config: &ImapConnectionConfig,
    mailbox_name: &str,
    location: &ImapMessageLocation,
    command: &SetKeywordsCommand,
) -> Result<MutationOutcome, ImapAdapterError> {
    let mut client = connect_authenticated_client(config).await?;
    let selected = selected_mailbox_from_examine(mailbox_name, client.select(mailbox_name).await?)?;
    if selected.uid_validity != location.uid_validity {
        return Err(ImapAdapterError::UidValidityMismatch {
            mailbox_name: mailbox_name.to_string(),
            expected: location.uid_validity.0,
            actual: selected.uid_validity.0,
        });
    }

    let uid_set = uid_sequence_set(location)?;
    let add_flags = imap_flags_for_keywords(&command.add)?;
    let remove_flags = imap_flags_for_keywords(&command.remove)?;
    if !add_flags.is_empty() {
        let items = client
            .uid_store(uid_set.clone(), StoreType::Add, add_flags)
            .await
            .map_err(ImapAdapterError::from)?;
        verify_uid_store_response(location, items.into_values().flatten())?;
    }
    if !remove_flags.is_empty() {
        let items = client
            .uid_store(uid_set, StoreType::Remove, remove_flags)
            .await
            .map_err(ImapAdapterError::from)?;
        verify_uid_store_response(location, items.into_values().flatten())?;
    }

    Ok(MutationOutcome { cursor: None })
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

fn verify_uid_store_response(
    location: &ImapMessageLocation,
    items: impl IntoIterator<Item = MessageDataItem<'static>>,
) -> Result<(), ImapAdapterError> {
    let found_matching_uid = items.into_iter().any(|item| match item {
        MessageDataItem::Uid(uid) => uid.get() == location.uid.0,
        _ => false,
    });
    if found_matching_uid {
        Ok(())
    } else {
        Err(ImapAdapterError::MissingFetchData(
            "matching UID STORE response",
        ))
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
    fn verifies_uid_store_response_contains_matching_uid() {
        let location = location();

        verify_uid_store_response(
            &location,
            [MessageDataItem::Uid(NonZeroU32::new(42).expect("uid"))],
        )
        .expect("matching UID");
    }

    #[test]
    fn rejects_uid_store_response_without_matching_uid() {
        let error = verify_uid_store_response(
            &location(),
            [MessageDataItem::Uid(NonZeroU32::new(99).expect("uid"))],
        )
        .expect_err("matching UID is required");

        assert!(matches!(
            error,
            ImapAdapterError::MissingFetchData("matching UID STORE response")
        ));
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
