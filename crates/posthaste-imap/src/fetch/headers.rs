use super::items::{fetch_item_names, fetched_header_from_items_with_metadata};
use super::*;

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
    let selected = examine_selected_mailbox(client, mailbox_name).await?;
    let mut uids =
        crate::timeout::with_deadline("uid_search", client.uid_search([SearchKey::Undeleted]))
            .await?;

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
pub(crate) async fn fetch_mailbox_headers_after_uid_with_client(
    client: &mut ImapClient,
    mailbox_name: &str,
    after_uid: ImapUid,
    fetch_modseq: bool,
    fetch_gmail_metadata: bool,
    updated_at: String,
) -> Result<ImapMailboxUidDeltaSnapshot, ImapAdapterError> {
    let selected = examine_selected_mailbox(client, mailbox_name).await?;
    let mut uids =
        crate::timeout::with_deadline("uid_search", client.uid_search([SearchKey::Undeleted]))
            .await?;

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
        let responses = crate::timeout::with_deadline(
            "uid_fetch",
            client.uid_fetch(
                sequence_set,
                fetch_item_names(fetch_modseq, fetch_gmail_metadata),
            ),
        )
        .await?;

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
