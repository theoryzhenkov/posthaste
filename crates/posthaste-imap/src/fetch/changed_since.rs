use super::items::{fetch_item_names, fetched_header_from_items_with_metadata};
use super::*;

/// Fetch message headers and flags changed since a stored MODSEQ.
///
/// When `include_vanished` is set, this issues `ENABLE QRESYNC` before the
/// `UID FETCH ... (CHANGEDSINCE ... VANISHED)` command, as required by RFC
/// 7162. Callers must treat returned headers as a partial update set, not as an
/// authoritative mailbox snapshot.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
/// @spec docs/L1-sync#syncbatch-and-apply_sync_batch
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
        let mut uids =
            crate::timeout::with_deadline("uid_search", client.uid_search([SearchKey::Undeleted]))
                .await?;
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
    let snapshot = crate::timeout::with_deadline_resolve(
        "changed_since_fetch",
        client.resolve(ChangedSinceFetchTask::new(
            sequence_set,
            fetch_item_names(true, fetch_gmail_metadata),
            since_modseq,
            fetch_vanished,
        )),
    )
    .await?;

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

async fn enable_qresync(client: &mut ImapClient) -> Result<bool, ImapAdapterError> {
    let capability = CapabilityEnable::try_from("QRESYNC")
        .map_err(|error| ImapAdapterError::Client(error.to_string()))?;
    let enabled = crate::timeout::with_deadline("enable", client.enable([capability]))
        .await?;

    Ok(enabled
        .unwrap_or_default()
        .iter()
        .any(|capability| capability.to_string().eq_ignore_ascii_case("QRESYNC")))
}

#[derive(Clone, Debug)]
pub(crate) struct ChangedSinceFetchSnapshot {
    pub(crate) headers: HashMap<NonZeroU32, Vec<MessageDataItem<'static>>>,
    pub(crate) vanished_uids: Vec<ImapUid>,
}

#[derive(Clone, Debug)]
pub(crate) struct ChangedSinceFetchTask {
    sequence_set: SequenceSet,
    macro_or_item_names: MacroOrMessageDataItemNames<'static>,
    since_modseq: NonZeroU64,
    include_vanished: bool,
    output: HashMap<NonZeroU32, HashSet<MessageDataItem<'static>>>,
    vanished_uids: Vec<ImapUid>,
}

impl ChangedSinceFetchTask {
    pub(crate) fn new(
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
