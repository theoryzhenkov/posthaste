use super::items::{fetch_item_names, fetched_header_from_items_with_metadata};
use super::*;

/// EXAMINE a mailbox, then search + normalize its undeleted UID set.
///
/// The search output is sorted ascending and deduped so downstream chunking and
/// sync reconciliation (including the B4 resumable initial-snapshot cursor) do
/// not depend on provider-specific ordering or duplicate behavior. Shared by the
/// full-snapshot, UID-delta, and streamed initial-snapshot paths.
pub(crate) async fn search_undeleted_uids(
    client: &mut ImapClient,
    mailbox_name: &str,
    fetch_modseq: bool,
) -> Result<(ImapSelectedMailbox, Vec<NonZeroU32>), ImapAdapterError> {
    let selected = examine_selected_mailbox(client, mailbox_name).await?;
    let mut uids =
        crate::timeout::with_deadline("uid_search", client.uid_search([SearchKey::Undeleted]))
            .await?;
    uids.sort_unstable();
    uids.dedup();
    ph_info!(
        events::IMAP_MAILBOX_UID_SEARCH_COMPLETED,
        mailbox_id = %selected.mailbox_id,
        uid_count = uids.len(),
        fetch_modseq,
        "IMAP mailbox UID search completed"
    );
    Ok((selected, uids))
}

/// Fetch header-level records for a single UID chunk of an already-selected
/// mailbox. Factored out of [`fetch_selected_mailbox_headers`] so the streamed
/// resumable initial snapshot (B4) can commit each chunk before fetching the
/// next, sharing the exact per-chunk fetch + projection logic.
pub(crate) async fn fetch_header_chunk(
    client: &mut ImapClient,
    selected: &ImapSelectedMailbox,
    chunk: &[NonZeroU32],
    fetch_modseq: bool,
    fetch_gmail_metadata: bool,
    updated_at: &str,
) -> Result<Vec<ImapMappedHeader>, ImapAdapterError> {
    let sequence_set = SequenceSet::try_from(chunk)
        .map_err(|error| ImapAdapterError::InvalidUidSequence(error.to_string()))?;
    let responses = crate::timeout::with_deadline(
        "uid_fetch",
        client.uid_fetch(
            sequence_set,
            fetch_item_names(fetch_modseq, fetch_gmail_metadata),
        ),
    )
    .await?;

    let mut records = Vec::with_capacity(responses.len());
    for items in responses.into_values() {
        let fetched =
            fetched_header_from_items_with_metadata(selected, items, updated_at.to_string())?;
        records.push(imap_header_message_record_with_gmail_metadata(
            selected,
            fetched.header,
            fetched.gmail,
        )?);
    }
    Ok(records)
}

/// Fetch selected mailbox state plus header-level records for every message in
/// one IMAP mailbox.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
/// @spec docs/L1-sync#body-lazy
pub(crate) async fn fetch_mailbox_header_snapshot_with_client(
    client: &mut ImapClient,
    mailbox_name: &str,
    fetch_modseq: bool,
    fetch_gmail_metadata: bool,
    updated_at: String,
) -> Result<ImapMailboxHeaderSnapshot, ImapAdapterError> {
    let (selected, uids) = search_undeleted_uids(client, mailbox_name, fetch_modseq).await?;

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
pub(crate) async fn fetch_mailbox_headers_after_uid_with_client(
    client: &mut ImapClient,
    mailbox_name: &str,
    after_uid: ImapUid,
    fetch_modseq: bool,
    fetch_gmail_metadata: bool,
    updated_at: String,
) -> Result<ImapMailboxUidDeltaSnapshot, ImapAdapterError> {
    let (selected, uids) = search_undeleted_uids(client, mailbox_name, fetch_modseq).await?;
    let current_uids = uids
        .iter()
        .map(|uid| ImapUid(uid.get()))
        .collect::<Vec<_>>();
    let new_uids = uids_above(&uids, after_uid);
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
        let chunk_records = fetch_header_chunk(
            client,
            selected,
            chunk,
            fetch_modseq,
            fetch_gmail_metadata,
            &updated_at,
        )
        .await?;
        records.extend(chunk_records);
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

/// The subset of `uids` strictly above `after_uid`, preserving order.
///
/// The resumable initial snapshot (B4) resumes from `after_uid =
/// partial_initial_uid`: because the committed prefix is exactly the UIDs at or
/// below the checkpoint, filtering here yields only the not-yet-fetched tail, so
/// a resumed sync re-fetches nothing already committed and skips nothing. On a
/// fresh initial sync `after_uid` is `0`, so every UID is returned.
pub(crate) fn uids_above(uids: &[NonZeroU32], after_uid: ImapUid) -> Vec<NonZeroU32> {
    uids.iter()
        .copied()
        .filter(|uid| uid.get() > after_uid.0)
        .collect()
}

#[cfg(test)]
mod uid_resume_tests {
    use super::*;

    fn uid(value: u32) -> NonZeroU32 {
        NonZeroU32::new(value).expect("nonzero uid")
    }

    #[test]
    fn resume_returns_only_the_tail_above_the_checkpoint() {
        // Interrupted after committing chunks whose highest UID is 20: a resume
        // from that checkpoint re-fetches nothing at or below it and returns
        // exactly the not-yet-fetched tail.
        let all = vec![uid(5), uid(10), uid(20), uid(21), uid(30)];
        assert_eq!(uids_above(&all, ImapUid(20)), vec![uid(21), uid(30)]);
    }

    #[test]
    fn fresh_initial_sync_returns_every_uid() {
        let all = vec![uid(5), uid(10), uid(20)];
        assert_eq!(uids_above(&all, ImapUid(0)), all);
    }

    #[test]
    fn caught_up_resume_returns_nothing() {
        let all = vec![uid(5), uid(10), uid(20)];
        assert!(uids_above(&all, ImapUid(20)).is_empty());
    }
}
