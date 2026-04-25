use std::{
    collections::{HashMap, HashSet},
    num::{NonZeroU32, NonZeroU64},
};

use imap_client::client::tokio::Client as ImapClient;
use imap_client::imap_types::command::{CommandBody, FetchModifier};
use imap_client::imap_types::extensions::enable::CapabilityEnable;
use imap_client::imap_types::fetch::{
    MacroOrMessageDataItemNames, MessageDataItem, MessageDataItemName,
};
use imap_client::imap_types::flag::FlagFetch;
use imap_client::imap_types::response::{Data, StatusBody, StatusKind};
use imap_client::imap_types::search::SearchKey;
use imap_client::imap_types::sequence::SequenceSet;
use imap_client::tasks::tasks::TaskError;
use imap_client::tasks::Task;
use posthaste_domain::{ImapModSeq, ImapSelectedMailbox, ImapUid};

use crate::discovery::connect_authenticated_client;
use crate::mailbox::examine_selected_mailbox;
use crate::{
    imap_header_message_record, normalize_imap_capabilities, ImapAdapterError,
    ImapConnectionConfig, ImapFetchedHeader, ImapMappedHeader,
};

const UID_FETCH_CHUNK_SIZE: usize = 128;

/// Header snapshot for one selected IMAP mailbox.
#[derive(Clone, Debug)]
pub struct ImapMailboxHeaderSnapshot {
    pub selected: ImapSelectedMailbox,
    pub headers: Vec<ImapMappedHeader>,
}

/// Header-level records changed since a previously stored mailbox MODSEQ.
///
/// `headers` is intentionally partial: it contains only messages returned by
/// `UID FETCH ... (CHANGEDSINCE ...)`. Deletions are carried separately through
/// `vanished_uids` when the server supports QRESYNC.
#[derive(Clone, Debug)]
pub struct ImapChangedSinceSnapshot {
    pub selected: ImapSelectedMailbox,
    pub headers: Vec<ImapMappedHeader>,
    pub vanished_uids: Vec<ImapUid>,
    pub is_full_snapshot: bool,
}

/// Fetch and map header-level records for every message in one IMAP mailbox.
///
/// This performs a conservative full mailbox snapshot: `UID SEARCH ALL` obtains
/// candidate UIDs, then chunked `UID FETCH` retrieves only metadata and
/// RFC822 headers. Message bodies remain lazy.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
/// @spec docs/L1-sync#body-lazy
pub async fn fetch_mailbox_header_records(
    config: &ImapConnectionConfig,
    mailbox_name: &str,
    updated_at: String,
) -> Result<Vec<ImapMappedHeader>, ImapAdapterError> {
    Ok(
        fetch_mailbox_header_snapshot(config, mailbox_name, updated_at)
            .await?
            .headers,
    )
}

/// Fetch selected mailbox state plus header-level records for every message in
/// one IMAP mailbox.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
/// @spec docs/L1-sync#body-lazy
pub async fn fetch_mailbox_header_snapshot(
    config: &ImapConnectionConfig,
    mailbox_name: &str,
    updated_at: String,
) -> Result<ImapMailboxHeaderSnapshot, ImapAdapterError> {
    let mut client = connect_authenticated_client(config).await?;
    client.refresh_capabilities().await?;
    let fetch_modseq = normalize_imap_capabilities(
        client
            .state
            .capabilities_iter()
            .map(std::string::ToString::to_string),
    )
    .supports_condstore();
    let selected = examine_selected_mailbox(&mut client, mailbox_name).await?;
    let mut uids = client.uid_search([SearchKey::All]).await?;

    // Normalize search output before chunking so later sync reconciliation does
    // not depend on provider-specific ordering or duplicate behavior.
    uids.sort_unstable();
    uids.dedup();

    let headers =
        fetch_selected_mailbox_headers(&mut client, &selected, &uids, fetch_modseq, updated_at)
            .await?;

    Ok(ImapMailboxHeaderSnapshot { selected, headers })
}

/// Fetch message headers and flags changed since a stored MODSEQ.
///
/// When `include_vanished` is set, this issues `ENABLE QRESYNC` before the
/// `UID FETCH ... (CHANGEDSINCE ... VANISHED)` command, as required by RFC
/// 7162. Callers must treat returned headers as a partial update set, not as an
/// authoritative mailbox snapshot.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
/// @spec docs/L1-sync#syncbatch-and-apply_sync_batch
pub async fn fetch_mailbox_changed_since_snapshot(
    config: &ImapConnectionConfig,
    mailbox_name: &str,
    since_modseq: ImapModSeq,
    include_vanished: bool,
    updated_at: String,
) -> Result<ImapChangedSinceSnapshot, ImapAdapterError> {
    let since_modseq =
        NonZeroU64::new(since_modseq.0).ok_or(ImapAdapterError::InvalidModSeq(since_modseq.0))?;
    let mut client = connect_authenticated_client(config).await?;
    client.refresh_capabilities().await?;
    let fetch_vanished = include_vanished && enable_qresync(&mut client).await?;
    let selected = examine_selected_mailbox(&mut client, mailbox_name).await?;
    if include_vanished && !fetch_vanished {
        let mut uids = client.uid_search([SearchKey::All]).await?;
        uids.sort_unstable();
        uids.dedup();
        let headers =
            fetch_selected_mailbox_headers(&mut client, &selected, &uids, true, updated_at).await?;

        return Ok(ImapChangedSinceSnapshot {
            selected,
            headers,
            vanished_uids: Vec::new(),
            is_full_snapshot: true,
        });
    }
    let sequence_set = SequenceSet::try_from("1:*")
        .map_err(|error| ImapAdapterError::InvalidUidSequence(error.to_string()))?;
    let snapshot = client
        .resolve(ChangedSinceFetchTask::new(
            sequence_set,
            fetch_item_names(true),
            since_modseq,
            fetch_vanished,
        ))
        .await
        .map_err(ImapAdapterError::from)?
        .map_err(|error| ImapAdapterError::Client(error.to_string()))?;

    let mut headers = Vec::with_capacity(snapshot.headers.len());
    for items in snapshot.headers.into_values() {
        let fetched = fetched_header_from_items(&selected, items, updated_at.clone())?;
        headers.push(imap_header_message_record(&selected, fetched)?);
    }
    headers.sort_by_key(|record| record.location.uid);

    Ok(ImapChangedSinceSnapshot {
        selected,
        headers,
        vanished_uids: snapshot.vanished_uids,
        is_full_snapshot: false,
    })
}

pub(crate) async fn fetch_selected_mailbox_headers(
    client: &mut ImapClient,
    selected: &ImapSelectedMailbox,
    uids: &[NonZeroU32],
    fetch_modseq: bool,
    updated_at: String,
) -> Result<Vec<ImapMappedHeader>, ImapAdapterError> {
    let mut records = Vec::new();
    for chunk in uids.chunks(UID_FETCH_CHUNK_SIZE) {
        let sequence_set = SequenceSet::try_from(chunk)
            .map_err(|error| ImapAdapterError::InvalidUidSequence(error.to_string()))?;
        let responses = client
            .uid_fetch(sequence_set, fetch_item_names(fetch_modseq))
            .await
            .map_err(ImapAdapterError::from)?;

        for items in responses.into_values() {
            let fetched = fetched_header_from_items(selected, items, updated_at.clone())?;
            records.push(imap_header_message_record(selected, fetched)?);
        }
    }

    records.sort_by_key(|record| record.location.uid);
    Ok(records)
}

async fn enable_qresync(client: &mut ImapClient) -> Result<bool, ImapAdapterError> {
    let capability = CapabilityEnable::try_from("QRESYNC")
        .map_err(|error| ImapAdapterError::Client(error.to_string()))?;
    let enabled = client
        .enable([capability])
        .await
        .map_err(ImapAdapterError::from)?;

    Ok(enabled
        .unwrap_or_default()
        .iter()
        .any(|capability| capability.to_string().eq_ignore_ascii_case("QRESYNC")))
}

fn fetch_item_names(fetch_modseq: bool) -> MacroOrMessageDataItemNames<'static> {
    let mut items = vec![
        MessageDataItemName::Flags,
        MessageDataItemName::Rfc822Header,
        MessageDataItemName::Rfc822Size,
        MessageDataItemName::Uid,
    ];
    if fetch_modseq {
        items.push(MessageDataItemName::ModSeq);
    }

    MacroOrMessageDataItemNames::MessageDataItemNames(items)
}

#[derive(Clone, Debug)]
struct ChangedSinceFetchSnapshot {
    headers: HashMap<NonZeroU32, Vec<MessageDataItem<'static>>>,
    vanished_uids: Vec<ImapUid>,
}

#[derive(Clone, Debug)]
struct ChangedSinceFetchTask {
    sequence_set: SequenceSet,
    macro_or_item_names: MacroOrMessageDataItemNames<'static>,
    since_modseq: NonZeroU64,
    include_vanished: bool,
    output: HashMap<NonZeroU32, HashSet<MessageDataItem<'static>>>,
    vanished_uids: Vec<ImapUid>,
}

impl ChangedSinceFetchTask {
    fn new(
        sequence_set: SequenceSet,
        macro_or_item_names: MacroOrMessageDataItemNames<'static>,
        since_modseq: NonZeroU64,
        include_vanished: bool,
    ) -> Self {
        Self {
            sequence_set,
            macro_or_item_names,
            since_modseq,
            include_vanished,
            output: HashMap::new(),
            vanished_uids: Vec::new(),
        }
    }
}

impl Task for ChangedSinceFetchTask {
    type Output = Result<ChangedSinceFetchSnapshot, TaskError>;

    fn command_body(&self) -> CommandBody<'static> {
        let mut modifiers = vec![FetchModifier::ChangedSince(self.since_modseq)];
        if self.include_vanished {
            modifiers.push(FetchModifier::Vanished);
        }

        CommandBody::Fetch {
            sequence_set: self.sequence_set.clone(),
            macro_or_item_names: self.macro_or_item_names.clone(),
            uid: true,
            modifiers,
        }
    }

    fn process_data(&mut self, data: Data<'static>) -> Option<Data<'static>> {
        match data {
            Data::Fetch { items, seq } => {
                if let Some(prev_items) = self.output.get_mut(&seq) {
                    prev_items.extend(items.into_iter());
                } else {
                    self.output.insert(seq, items.into_iter().collect());
                }
                None
            }
            Data::Vanished { known_uids, .. } => {
                self.vanished_uids.extend(
                    known_uids
                        .iter(NonZeroU32::MAX)
                        .map(|uid| ImapUid(uid.get())),
                );
                None
            }
            other => Some(other),
        }
    }

    fn process_tagged(self, status_body: StatusBody<'static>) -> Self::Output {
        match status_body.kind {
            StatusKind::Ok => Ok(ChangedSinceFetchSnapshot {
                headers: self
                    .output
                    .into_iter()
                    .map(|(seq, items)| (seq, items.into_iter().collect()))
                    .collect(),
                vanished_uids: self.vanished_uids,
            }),
            StatusKind::No => Err(TaskError::UnexpectedNoResponse(status_body)),
            StatusKind::Bad => Err(TaskError::UnexpectedBadResponse(status_body)),
        }
    }
}

/// Extract the IMAP data items needed by Posthaste from one FETCH response.
///
/// `imap-client` returns FETCH rows keyed by sequence number even for
/// `UID FETCH`; this function always takes identity from the `UID` data item.
pub fn fetched_header_from_items(
    selected: &ImapSelectedMailbox,
    items: impl IntoIterator<Item = MessageDataItem<'static>>,
    updated_at: String,
) -> Result<ImapFetchedHeader, ImapAdapterError> {
    let mut uid = None;
    let mut modseq = None;
    let mut flags = Vec::new();
    let mut rfc822_size = None;
    let mut headers = None;

    for item in items {
        match item {
            MessageDataItem::Flags(next_flags) => {
                flags = next_flags.into_iter().map(imap_flag_fetch_name).collect();
            }
            MessageDataItem::Rfc822Header(nstring) => {
                headers = Some(
                    nstring
                        .into_option()
                        .map(|header| header.into_owned())
                        .unwrap_or_default(),
                );
            }
            MessageDataItem::Rfc822Size(size) => {
                rfc822_size = Some(i64::from(size));
            }
            MessageDataItem::Uid(next_uid) => {
                uid = Some(ImapUid(next_uid.get()));
            }
            MessageDataItem::ModSeq(next_modseq) => {
                modseq = Some(ImapModSeq(next_modseq.get()));
            }
            _ => {}
        }
    }

    Ok(ImapFetchedHeader {
        mailbox_id: selected.mailbox_id.clone(),
        uid: uid.ok_or(ImapAdapterError::MissingFetchData("UID"))?,
        modseq,
        flags,
        rfc822_size: rfc822_size.ok_or(ImapAdapterError::MissingFetchData("RFC822.SIZE"))?,
        headers: headers.ok_or(ImapAdapterError::MissingFetchData("RFC822.HEADER"))?,
        updated_at,
    })
}

fn imap_flag_fetch_name(flag: FlagFetch<'static>) -> String {
    match flag {
        FlagFetch::Flag(flag) => flag.to_string(),
        FlagFetch::Recent => "\\Recent".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::num::{NonZeroU32, NonZeroU64};

    use imap_client::imap_types::core::{NString, Text};
    use imap_client::imap_types::flag::{Flag, FlagFetch};
    use posthaste_domain::{ImapUidValidity, MailboxId};

    use super::*;

    fn selected_mailbox() -> ImapSelectedMailbox {
        ImapSelectedMailbox {
            mailbox_id: MailboxId::from("imap:mailbox:494e424f58"),
            mailbox_name: "INBOX".to_string(),
            uid_validity: ImapUidValidity(9),
            uid_next: None,
            highest_modseq: None,
        }
    }

    #[test]
    fn fetched_header_extracts_required_items_and_modseq() {
        let fetched = fetched_header_from_items(
            &selected_mailbox(),
            [
                MessageDataItem::Flags(vec![
                    FlagFetch::Flag(Flag::Seen),
                    FlagFetch::Flag(Flag::Flagged),
                    FlagFetch::Recent,
                ]),
                MessageDataItem::Rfc822Header(
                    NString::try_from(
                        b"From: Alice <alice@example.test>\r\nSubject: Hello\r\n\r\n".as_slice(),
                    )
                    .expect("header nstring"),
                ),
                MessageDataItem::Rfc822Size(512),
                MessageDataItem::Uid(NonZeroU32::new(42).expect("uid")),
                MessageDataItem::ModSeq(NonZeroU64::new(777).expect("modseq")),
            ],
            "2026-04-25T00:00:00Z".to_string(),
        )
        .expect("fetched header");

        assert_eq!(fetched.uid, ImapUid(42));
        assert_eq!(fetched.modseq, Some(ImapModSeq(777)));
        assert_eq!(
            fetched.flags,
            vec![
                "\\Seen".to_string(),
                "\\Flagged".to_string(),
                "\\Recent".to_string()
            ]
        );
        assert_eq!(fetched.rfc822_size, 512);
        assert!(fetched.headers.starts_with(b"From: Alice"));
    }

    #[test]
    fn fetched_header_requires_uid() {
        let error = fetched_header_from_items(
            &selected_mailbox(),
            [
                MessageDataItem::Rfc822Header(
                    NString::try_from(b"Subject: Hello\r\n\r\n".as_slice())
                        .expect("header nstring"),
                ),
                MessageDataItem::Rfc822Size(512),
            ],
            "2026-04-25T00:00:00Z".to_string(),
        )
        .expect_err("UID is required");

        assert!(matches!(error, ImapAdapterError::MissingFetchData("UID")));
    }

    #[test]
    fn fetch_items_only_include_modseq_when_condstore_is_available() {
        assert_eq!(
            fetch_item_names(false),
            MacroOrMessageDataItemNames::MessageDataItemNames(vec![
                MessageDataItemName::Flags,
                MessageDataItemName::Rfc822Header,
                MessageDataItemName::Rfc822Size,
                MessageDataItemName::Uid,
            ])
        );
        assert_eq!(
            fetch_item_names(true),
            MacroOrMessageDataItemNames::MessageDataItemNames(vec![
                MessageDataItemName::Flags,
                MessageDataItemName::Rfc822Header,
                MessageDataItemName::Rfc822Size,
                MessageDataItemName::Uid,
                MessageDataItemName::ModSeq,
            ])
        );
    }

    #[test]
    fn changed_since_fetch_task_uses_uid_fetch_modifiers() {
        let task = ChangedSinceFetchTask::new(
            SequenceSet::try_from("1:*").expect("sequence set"),
            fetch_item_names(true),
            NonZeroU64::new(777).expect("modseq"),
            true,
        );

        let CommandBody::Fetch {
            uid,
            modifiers,
            macro_or_item_names,
            ..
        } = task.command_body()
        else {
            panic!("expected FETCH");
        };

        assert!(uid);
        assert_eq!(
            modifiers,
            vec![
                FetchModifier::ChangedSince(NonZeroU64::new(777).expect("modseq")),
                FetchModifier::Vanished,
            ]
        );
        assert_eq!(macro_or_item_names, fetch_item_names(true));
    }

    #[test]
    fn changed_since_fetch_task_collects_fetch_rows_and_vanished_uids() {
        let mut task = ChangedSinceFetchTask::new(
            SequenceSet::try_from("1:*").expect("sequence set"),
            fetch_item_names(true),
            NonZeroU64::new(777).expect("modseq"),
            true,
        );

        assert!(task
            .process_data(Data::Fetch {
                seq: NonZeroU32::new(2).expect("seq"),
                items: vec![MessageDataItem::Uid(NonZeroU32::new(42).expect("uid"))]
                    .try_into()
                    .expect("fetch items"),
            })
            .is_none());
        assert!(task
            .process_data(Data::Vanished {
                earlier: true,
                known_uids: SequenceSet::try_from("7:8,10").expect("vanished uids"),
            })
            .is_none());

        let snapshot = task
            .process_tagged(StatusBody {
                kind: StatusKind::Ok,
                code: None,
                text: Text::unvalidated("FETCH completed"),
            })
            .expect("fetch snapshot");

        assert_eq!(snapshot.headers.len(), 1);
        assert_eq!(
            snapshot.vanished_uids,
            vec![ImapUid(7), ImapUid(8), ImapUid(10)]
        );
    }
}
