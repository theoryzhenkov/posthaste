use std::{
    collections::{HashMap, HashSet},
    num::{NonZeroU32, NonZeroU64},
};

use imap_client::client::tokio::Client as ImapClient;
use imap_client::imap_types::body::{BodyStructure, SpecificFields};
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
use posthaste_domain::{
    GmailLabel, GmailMessageId, GmailThreadId, ImapGmailMetadata, ImapModSeq, ImapSelectedMailbox,
    ImapUid,
};
use posthaste_observability::{events, ph_debug, ph_info};

use crate::discovery::connect_authenticated_client;
use crate::mailbox::examine_selected_mailbox;
use crate::message::imap_flags_include_deleted;
use crate::{
    imap_header_message_record_with_gmail_metadata, normalize_imap_capabilities, ImapAdapterError,
    ImapConnectionConfig, ImapFetchedHeader, ImapMappedHeader,
};

const UID_FETCH_CHUNK_SIZE: usize = 128;

/// Header snapshot for one selected IMAP mailbox.
#[derive(Clone, Debug)]
pub struct ImapMailboxHeaderSnapshot {
    pub selected: ImapSelectedMailbox,
    pub headers: Vec<ImapMappedHeader>,
}

/// Header-level delta for mailboxes where UID is the only available sync state.
///
/// `current_uids` is an authoritative UID listing for deletion reconciliation,
/// while `headers` only contains records newer than the stored UID watermark.
#[derive(Clone, Debug)]
pub struct ImapMailboxUidDeltaSnapshot {
    pub selected: ImapSelectedMailbox,
    pub headers: Vec<ImapMappedHeader>,
    pub current_uids: Vec<ImapUid>,
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
    let capabilities = normalize_imap_capabilities(
        client
            .state
            .capabilities_iter()
            .map(std::string::ToString::to_string),
    );
    let fetch_modseq = capabilities.supports_condstore();
    let fetch_gmail_metadata = capabilities.supports_gmail_extensions();
    fetch_mailbox_header_snapshot_with_client(
        &mut client,
        mailbox_name,
        fetch_modseq,
        fetch_gmail_metadata,
        updated_at,
    )
    .await
}

pub(crate) async fn fetch_mailbox_header_snapshot_with_client(
    client: &mut ImapClient,
    mailbox_name: &str,
    fetch_modseq: bool,
    fetch_gmail_metadata: bool,
    updated_at: String,
) -> Result<ImapMailboxHeaderSnapshot, ImapAdapterError> {
    let selected = examine_selected_mailbox(client, mailbox_name).await?;
    let mut uids = client.uid_search([SearchKey::Undeleted]).await?;

    // Normalize search output before chunking so later sync reconciliation does
    // not depend on provider-specific ordering or duplicate behavior.
    uids.sort_unstable();
    uids.dedup();
    ph_info!(
        events::IMAP_MAILBOX_UID_SEARCH_COMPLETED,
        mailbox_id = %selected.mailbox_id,
        uid_count = uids.len(),
        fetch_modseq,
        "IMAP mailbox UID search completed"
    );

    let headers = fetch_selected_mailbox_headers(
        client,
        &selected,
        &uids,
        fetch_modseq,
        fetch_gmail_metadata,
        updated_at,
    )
    .await?;

    Ok(ImapMailboxHeaderSnapshot { selected, headers })
}

/// Fetch headers for messages whose UID is above the stored watermark.
///
/// RFC 3501/9051 UID ranges with `*` can include the highest existing UID even
/// when the lower bound is above all assigned UIDs, so this path searches all
/// UIDs and filters client-side instead of issuing `UID SEARCH UID n:*`.
///
/// @spec docs/L0-providers#imap-delta-fallback
/// @spec docs/L1-sync#body-lazy
pub async fn fetch_mailbox_headers_after_uid(
    config: &ImapConnectionConfig,
    mailbox_name: &str,
    after_uid: ImapUid,
    updated_at: String,
) -> Result<ImapMailboxUidDeltaSnapshot, ImapAdapterError> {
    let mut client = connect_authenticated_client(config).await?;
    client.refresh_capabilities().await?;
    let capabilities = normalize_imap_capabilities(
        client
            .state
            .capabilities_iter()
            .map(std::string::ToString::to_string),
    );
    let fetch_modseq = capabilities.supports_condstore();
    let fetch_gmail_metadata = capabilities.supports_gmail_extensions();
    fetch_mailbox_headers_after_uid_with_client(
        &mut client,
        mailbox_name,
        after_uid,
        fetch_modseq,
        fetch_gmail_metadata,
        updated_at,
    )
    .await
}

pub(crate) async fn fetch_mailbox_headers_after_uid_with_client(
    client: &mut ImapClient,
    mailbox_name: &str,
    after_uid: ImapUid,
    fetch_modseq: bool,
    fetch_gmail_metadata: bool,
    updated_at: String,
) -> Result<ImapMailboxUidDeltaSnapshot, ImapAdapterError> {
    let selected = examine_selected_mailbox(client, mailbox_name).await?;
    let mut uids = client.uid_search([SearchKey::Undeleted]).await?;

    uids.sort_unstable();
    uids.dedup();
    let current_uids = uids
        .iter()
        .map(|uid| ImapUid(uid.get()))
        .collect::<Vec<_>>();
    let new_uids = uids
        .into_iter()
        .filter(|uid| uid.get() > after_uid.0)
        .collect::<Vec<_>>();
    ph_info!(
        events::IMAP_MAILBOX_UID_DELTA_SEARCH_COMPLETED,
        mailbox_id = %selected.mailbox_id,
        uid_count = current_uids.len(),
        new_uid_count = new_uids.len(),
        after_uid = after_uid.0,
        fetch_modseq,
        "IMAP mailbox UID delta search completed"
    );

    let headers = fetch_selected_mailbox_headers(
        client,
        &selected,
        &new_uids,
        fetch_modseq,
        fetch_gmail_metadata,
        updated_at,
    )
    .await?;

    Ok(ImapMailboxUidDeltaSnapshot {
        selected,
        headers,
        current_uids,
    })
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
    let mut client = connect_authenticated_client(config).await?;
    client.refresh_capabilities().await?;
    let fetch_gmail_metadata = normalize_imap_capabilities(
        client
            .state
            .capabilities_iter()
            .map(std::string::ToString::to_string),
    )
    .supports_gmail_extensions();
    fetch_mailbox_changed_since_snapshot_with_client(
        &mut client,
        mailbox_name,
        since_modseq,
        include_vanished,
        fetch_gmail_metadata,
        updated_at,
    )
    .await
}

pub(crate) async fn fetch_mailbox_changed_since_snapshot_with_client(
    client: &mut ImapClient,
    mailbox_name: &str,
    since_modseq: ImapModSeq,
    include_vanished: bool,
    fetch_gmail_metadata: bool,
    updated_at: String,
) -> Result<ImapChangedSinceSnapshot, ImapAdapterError> {
    let since_modseq =
        NonZeroU64::new(since_modseq.0).ok_or(ImapAdapterError::InvalidModSeq(since_modseq.0))?;
    let fetch_vanished = include_vanished && enable_qresync(client).await?;
    let selected = examine_selected_mailbox(client, mailbox_name).await?;
    if include_vanished && !fetch_vanished {
        let mut uids = client.uid_search([SearchKey::Undeleted]).await?;
        uids.sort_unstable();
        uids.dedup();
        ph_info!(
            events::IMAP_MAILBOX_UID_SEARCH_COMPLETED,
            mailbox_id = %selected.mailbox_id,
            uid_count = uids.len(),
            fetch_modseq = true,
            reason = "qresync_enable_unavailable",
            "IMAP mailbox UID search completed"
        );
        let headers = fetch_selected_mailbox_headers(
            client,
            &selected,
            &uids,
            true,
            fetch_gmail_metadata,
            updated_at,
        )
        .await?;

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
            fetch_item_names(true, fetch_gmail_metadata),
            since_modseq,
            fetch_vanished,
        ))
        .await
        .map_err(ImapAdapterError::from)?
        .map_err(|error| ImapAdapterError::Client(error.to_string()))?;

    let mut headers = Vec::with_capacity(snapshot.headers.len());
    let mut vanished_uids = snapshot.vanished_uids;
    for items in snapshot.headers.into_values() {
        let fetched =
            fetched_header_from_items_with_metadata(&selected, items, updated_at.clone())?;
        if imap_flags_include_deleted(&fetched.header.flags) {
            vanished_uids.push(fetched.header.uid);
            continue;
        }
        headers.push(imap_header_message_record_with_gmail_metadata(
            &selected,
            fetched.header,
            fetched.gmail,
        )?);
    }
    headers.sort_by_key(|record| record.location.uid);
    vanished_uids.sort();
    vanished_uids.dedup();

    Ok(ImapChangedSinceSnapshot {
        selected,
        headers,
        vanished_uids,
        is_full_snapshot: false,
    })
}

pub(crate) async fn fetch_selected_mailbox_headers(
    client: &mut ImapClient,
    selected: &ImapSelectedMailbox,
    uids: &[NonZeroU32],
    fetch_modseq: bool,
    fetch_gmail_metadata: bool,
    updated_at: String,
) -> Result<Vec<ImapMappedHeader>, ImapAdapterError> {
    let mut records = Vec::new();
    let chunk_count = uids.len().div_ceil(UID_FETCH_CHUNK_SIZE);
    for (chunk_index, chunk) in uids.chunks(UID_FETCH_CHUNK_SIZE).enumerate() {
        let sequence_set = SequenceSet::try_from(chunk)
            .map_err(|error| ImapAdapterError::InvalidUidSequence(error.to_string()))?;
        let responses = client
            .uid_fetch(
                sequence_set,
                fetch_item_names(fetch_modseq, fetch_gmail_metadata),
            )
            .await
            .map_err(ImapAdapterError::from)?;

        for items in responses.into_values() {
            let fetched =
                fetched_header_from_items_with_metadata(selected, items, updated_at.clone())?;
            records.push(imap_header_message_record_with_gmail_metadata(
                selected,
                fetched.header,
                fetched.gmail,
            )?);
        }
        ph_info!(
            events::IMAP_MAILBOX_HEADER_FETCH_PROGRESS,
            mailbox_id = %selected.mailbox_id,
            chunk_index = chunk_index + 1,
            chunk_count,
            fetched_count = records.len(),
            total_count = uids.len(),
            "IMAP mailbox header fetch progress"
        );
    }

    records.sort_by_key(|record| record.location.uid);
    ph_debug!(
        events::IMAP_MAILBOX_HEADER_FETCH_SORTED,
        mailbox_id = %selected.mailbox_id,
        fetched_count = records.len(),
        "IMAP mailbox header fetch sorted"
    );
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

fn fetch_item_names(
    fetch_modseq: bool,
    fetch_gmail_metadata: bool,
) -> MacroOrMessageDataItemNames<'static> {
    let mut items = vec![
        MessageDataItemName::Flags,
        MessageDataItemName::BodyStructure,
        MessageDataItemName::Rfc822Header,
        MessageDataItemName::Rfc822Size,
        MessageDataItemName::Uid,
    ];
    if fetch_modseq {
        items.push(MessageDataItemName::ModSeq);
    }
    if fetch_gmail_metadata {
        items.push(MessageDataItemName::GmailMessageId);
        items.push(MessageDataItemName::GmailThreadId);
        items.push(MessageDataItemName::GmailLabels);
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
                    prev_items.extend(items);
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
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImapFetchedHeaderWithMetadata {
    pub header: ImapFetchedHeader,
    pub gmail: ImapGmailMetadata,
}

pub fn fetched_header_from_items(
    selected: &ImapSelectedMailbox,
    items: impl IntoIterator<Item = MessageDataItem<'static>>,
    updated_at: String,
) -> Result<ImapFetchedHeader, ImapAdapterError> {
    Ok(fetched_header_from_items_with_metadata(selected, items, updated_at)?.header)
}

/// Extract Posthaste header data plus typed provider metadata from one FETCH
/// response.
///
/// Gmail fields are optional because generic IMAP fetches do not request them
/// and because RFC identity remains the fallback when Gmail omits them.
pub fn fetched_header_from_items_with_metadata(
    selected: &ImapSelectedMailbox,
    items: impl IntoIterator<Item = MessageDataItem<'static>>,
    updated_at: String,
) -> Result<ImapFetchedHeaderWithMetadata, ImapAdapterError> {
    let mut uid = None;
    let mut modseq = None;
    let mut gmail = ImapGmailMetadata::default();
    let mut flags = Vec::new();
    let mut rfc822_size = None;
    let mut has_attachment = false;
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
            MessageDataItem::BodyStructure(body_structure) => {
                has_attachment = body_structure_has_attachment(&body_structure);
            }
            MessageDataItem::Uid(next_uid) => {
                uid = Some(ImapUid(next_uid.get()));
            }
            MessageDataItem::ModSeq(next_modseq) => {
                modseq = Some(ImapModSeq(next_modseq.get()));
            }
            MessageDataItem::GmailMessageId(gmail_message_id) => {
                gmail.message_id = Some(GmailMessageId(gmail_message_id));
            }
            MessageDataItem::GmailThreadId(gmail_thread_id) => {
                gmail.thread_id = Some(GmailThreadId(gmail_thread_id));
            }
            MessageDataItem::GmailLabels(labels) => {
                gmail.labels_observed = true;
                gmail.labels = labels
                    .into_iter()
                    .map(|label| GmailLabel::from(label.as_ref()))
                    .collect();
            }
            _ => {}
        }
    }

    Ok(ImapFetchedHeaderWithMetadata {
        header: ImapFetchedHeader {
            mailbox_id: selected.mailbox_id.clone(),
            uid: uid.ok_or(ImapAdapterError::MissingFetchData("UID"))?,
            modseq,
            flags,
            rfc822_size: rfc822_size.ok_or(ImapAdapterError::MissingFetchData("RFC822.SIZE"))?,
            has_attachment,
            headers: headers.ok_or(ImapAdapterError::MissingFetchData("RFC822.HEADER"))?,
            updated_at,
        },
        gmail,
    })
}

fn body_structure_has_attachment(body_structure: &BodyStructure<'_>) -> bool {
    match body_structure {
        BodyStructure::Single {
            body,
            extension_data,
        } => {
            let has_attachment_disposition = extension_data
                .as_ref()
                .and_then(|extension| extension.tail.as_ref())
                .and_then(|disposition| disposition.disposition.as_ref())
                .is_some_and(|(kind, _)| kind.as_ref().eq_ignore_ascii_case(b"attachment"));
            let has_name_parameter = body
                .basic
                .parameter_list
                .iter()
                .any(|(key, _)| key.as_ref().eq_ignore_ascii_case(b"name"));
            let is_basic_non_text = matches!(
                &body.specific,
                SpecificFields::Basic { r#type, .. }
                    if !r#type.as_ref().eq_ignore_ascii_case(b"text")
            );

            has_attachment_disposition || has_name_parameter || is_basic_non_text
        }
        BodyStructure::Multi { bodies, .. } => {
            bodies.as_ref().iter().any(body_structure_has_attachment)
        }
    }
}

fn imap_flag_fetch_name(flag: FlagFetch<'static>) -> String {
    match flag {
        FlagFetch::Flag(flag) => flag.to_string(),
        FlagFetch::Recent => "\\Recent".to_string(),
    }
}

#[cfg(test)]
mod tests;
