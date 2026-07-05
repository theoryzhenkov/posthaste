use std::collections::BTreeSet;

use posthaste_domain_model::{AccountId, MailboxId};
use posthaste_domain_model::{
    ImapMailboxSyncState, ImapMessageLocation, ImapUid, ImapUidValidity, MailboxRecord, SyncBatch,
    SyncCursor, SyncObject,
};

use crate::{
    DiscoveredImapAccount, ImapChangedSinceSnapshot, ImapMailboxHeaderSnapshot, ImapMappedHeader,
};

mod cursors;
mod deletions;
mod projection;

use cursors::{mailbox_cursor_state, message_cursor_state};
use deletions::{
    deleted_locations_for_delta, deleted_locations_matching_vanished_uids,
    deleted_locations_missing_from_remote, deleted_message_ids_for_deleted_locations,
    partition_deleted_message_ids_by_origin, preserve_delta_mailboxes_from_locations,
};
use projection::{
    messages_and_locations_for_batch, messages_and_locations_from_headers, project_imap_headers,
    ProjectedMessages,
};

/// Convert an IMAP mailbox discovery result into an authoritative mailbox
/// snapshot. Message sync is intentionally separate because it depends on
/// per-mailbox UIDVALIDITY and UID fetch state.
///
/// @spec docs/L0-providers#imap-discovery-runtime
pub fn imap_mailbox_sync_batch(
    _account_id: &AccountId,
    discovery: DiscoveredImapAccount,
    updated_at: String,
) -> SyncBatch {
    let mailboxes = discovery
        .mailboxes
        .iter()
        .filter(|mailbox| mailbox.selectable)
        .map(|mailbox| MailboxRecord {
            id: mailbox.id.clone(),
            name: mailbox.name.clone(),
            role: mailbox.role.map(str::to_string),
            unread_emails: 0,
            total_emails: 0,
        })
        .collect::<Vec<_>>();
    let cursor_state = mailbox_cursor_state(&mailboxes);

    SyncBatch {
        mailboxes,
        messages: Vec::new(),
        imap_mailbox_states: Vec::new(),
        imap_message_locations: Vec::new(),
        deleted_imap_message_locations: Vec::new(),
        deleted_mailbox_ids: Vec::new(),
        deleted_message_ids: Vec::new(),
        absence_deleted_imap_message_locations: Vec::new(),
        absence_deleted_message_ids: Vec::new(),
        replace_all_mailboxes: true,
        replace_all_messages: false,
        cursors: vec![SyncCursor {
            object_type: SyncObject::Mailbox,
            state: cursor_state,
            updated_at,
        }],
    }
}

/// Convert IMAP discovery plus fetched mailbox headers into a full local
/// metadata snapshot.
///
/// The first IMAP sync path is intentionally full-snapshot based. UIDVALIDITY
/// and expunge handling make delta correctness mailbox-scoped; until that state
/// is wired through the runtime, the store's authoritative replacement contract
/// is the safer boundary.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
/// @spec docs/L1-sync#syncbatch-and-apply_sync_batch
pub fn imap_full_sync_batch(
    account_id: &AccountId,
    discovery: DiscoveredImapAccount,
    headers: Vec<ImapMappedHeader>,
    mailbox_states: Vec<ImapMailboxSyncState>,
    updated_at: String,
) -> SyncBatch {
    let ProjectedMessages {
        messages,
        locations,
        ..
    } = messages_and_locations_for_batch(&discovery, headers);
    let mut batch = imap_mailbox_sync_batch(account_id, discovery, updated_at.clone());

    batch.imap_mailbox_states = mailbox_states;
    batch.messages = messages;
    batch.imap_message_locations = locations;
    batch.replace_all_messages = true;
    batch.cursors.push(SyncCursor {
        object_type: SyncObject::Message,
        state: message_cursor_state(&batch.messages, &batch.imap_message_locations),
        updated_at,
    });
    batch
}

pub fn imap_delta_sync_batch(
    account_id: &AccountId,
    discovery: DiscoveredImapAccount,
    headers: Vec<ImapMappedHeader>,
    mailbox_states: Vec<ImapMailboxSyncState>,
    local_locations: Vec<ImapMessageLocation>,
    updated_at: String,
) -> SyncBatch {
    let headers = project_imap_headers(&discovery, headers);
    let remote_locations = headers
        .iter()
        .map(|header| header.location.key())
        .collect::<BTreeSet<_>>();
    let ProjectedMessages {
        mut messages,
        locations,
        provider_absent_mailbox_ids_by_message,
    } = messages_and_locations_from_headers(headers);
    let mut batch = imap_mailbox_sync_batch(account_id, discovery, updated_at.clone());
    let deleted_imap_message_locations = deleted_locations_for_delta(
        &local_locations,
        deleted_locations_missing_from_remote(&local_locations, &remote_locations),
        &provider_absent_mailbox_ids_by_message,
    );
    let deleted_message_ids = deleted_message_ids_for_deleted_locations(
        &local_locations,
        &deleted_imap_message_locations,
        &locations,
    );
    preserve_delta_mailboxes_from_locations(
        &mut messages,
        &local_locations,
        &deleted_imap_message_locations,
        &locations,
        &provider_absent_mailbox_ids_by_message,
    );

    batch.imap_mailbox_states = mailbox_states;
    batch.messages = messages;
    batch.imap_message_locations = locations;
    batch.deleted_imap_message_locations = deleted_imap_message_locations;
    batch.deleted_message_ids = deleted_message_ids;
    batch.replace_all_messages = false;
    batch.cursors.push(SyncCursor {
        object_type: SyncObject::Message,
        state: message_cursor_state(&batch.messages, &batch.imap_message_locations),
        updated_at,
    });
    batch
}

/// Build the partial (explicit-deletion) IMAP delta batch.
///
/// DP-C4 mail-loss discrimination: deletions carry their ORIGIN so the store can
/// floor-guard only the inferred ones.
///   - `vanished_uids` are AUTHORITATIVE — server-asserted QRESYNC
///     `VANISHED (EARLIER)`. Their location/message removals are applied
///     unconditionally (`deleted_imap_message_locations` / `deleted_message_ids`).
///   - `absence_uids` are INFERRED — a local UID that is absent from a
///     `UID SEARCH UNDELETED` / header snapshot that may have been truncated or
///     dropped. Together with provider-absent (label-removed) locations they land
///     in `absence_deleted_imap_message_locations` / `absence_deleted_message_ids`,
///     which the store routes through the DS1 floor guard.
///
/// A message counted as fully-gone is attributed to the authoritative bucket only
/// when every one of its removed locations is a VANISHED removal; otherwise the
/// removal depends on an inference and is floor-guarded.
#[allow(clippy::too_many_arguments)]
pub fn imap_condstore_delta_sync_batch(
    account_id: &AccountId,
    discovery: DiscoveredImapAccount,
    headers: Vec<ImapMappedHeader>,
    mailbox_states: Vec<ImapMailboxSyncState>,
    local_locations: Vec<ImapMessageLocation>,
    vanished_uids: Vec<(MailboxId, ImapUidValidity, ImapUid)>,
    absence_uids: Vec<(MailboxId, ImapUidValidity, ImapUid)>,
    updated_at: String,
) -> SyncBatch {
    let ProjectedMessages {
        mut messages,
        locations,
        provider_absent_mailbox_ids_by_message,
    } = messages_and_locations_for_batch(&discovery, headers);
    let mut batch = imap_mailbox_sync_batch(account_id, discovery, updated_at.clone());

    // Authoritative location removals: server-asserted VANISHED plus provider-
    // absent (Gmail label) removals. The latter are derived from a message's OWN
    // returned label set (the server returned that message and told us its
    // current labels), so they are an authoritative per-message assertion — not a
    // truncation-risk inference — and are applied unconditionally.
    let vanished_locations = vanished_uids.into_iter().collect::<BTreeSet<_>>();
    let authoritative_keys = deleted_locations_for_delta(
        &local_locations,
        deleted_locations_matching_vanished_uids(&local_locations, &vanished_locations),
        &provider_absent_mailbox_ids_by_message,
    );
    let authoritative_key_set = authoritative_keys.iter().cloned().collect::<BTreeSet<_>>();
    // Absence-derived removals: a local UID absent from a possibly-truncated
    // `UID SEARCH UNDELETED`. A location that is also authoritative stays so.
    let absence_locations = absence_uids.into_iter().collect::<BTreeSet<_>>();
    let absence_keys =
        deleted_locations_matching_vanished_uids(&local_locations, &absence_locations)
            .into_iter()
            .filter(|key| !authoritative_key_set.contains(key))
            .collect::<Vec<_>>();
    let absence_key_set = absence_keys.iter().cloned().collect::<BTreeSet<_>>();

    // Fully-gone message ids over the union of both removal sets, then partition
    // by origin.
    let union_keys = authoritative_keys
        .iter()
        .cloned()
        .chain(absence_keys.iter().cloned())
        .collect::<Vec<_>>();
    let fully_gone =
        deleted_message_ids_for_deleted_locations(&local_locations, &union_keys, &locations);
    let (deleted_message_ids, absence_deleted_message_ids) =
        partition_deleted_message_ids_by_origin(
            &local_locations,
            &fully_gone,
            &authoritative_key_set,
            &absence_key_set,
        );

    preserve_delta_mailboxes_from_locations(
        &mut messages,
        &local_locations,
        &union_keys,
        &locations,
        &provider_absent_mailbox_ids_by_message,
    );

    batch.imap_mailbox_states = mailbox_states;
    batch.messages = messages;
    batch.imap_message_locations = locations;
    batch.deleted_imap_message_locations = authoritative_keys;
    batch.deleted_message_ids = deleted_message_ids;
    batch.absence_deleted_imap_message_locations = absence_keys;
    batch.absence_deleted_message_ids = absence_deleted_message_ids;
    batch.replace_all_messages = false;
    batch.cursors.push(SyncCursor {
        object_type: SyncObject::Message,
        state: message_cursor_state(&batch.messages, &batch.imap_message_locations),
        updated_at,
    });
    batch
}

pub fn imap_mailbox_state_from_header_snapshot(
    snapshot: &ImapMailboxHeaderSnapshot,
    updated_at: String,
) -> ImapMailboxSyncState {
    ImapMailboxSyncState {
        mailbox_id: snapshot.selected.mailbox_id.clone(),
        mailbox_name: snapshot.selected.mailbox_name.clone(),
        uid_validity: snapshot.selected.uid_validity,
        highest_uid: snapshot
            .headers
            .iter()
            .map(|header| header.location.uid)
            .max(),
        // Prefer the authoritative SELECT/EXAMINE `[HIGHESTMODSEQ]` watermark
        // (RFC 7162) over the max of fetched per-message MODSEQs: an empty (or
        // canonically-deduped) mailbox fetches no MODSEQ-bearing headers, so
        // deriving from headers alone would store `None` — leaving the mailbox
        // unable to take the CONDSTORE/QRESYNC delta path, so it re-runs a full
        // snapshot on every sync. Fall back to the per-message max for servers
        // that omit HIGHESTMODSEQ from SELECT. (The delta path already does this
        // via `record_highest_modseq(selected.highest_modseq)`.)
        highest_modseq: snapshot.selected.highest_modseq.or_else(|| {
            snapshot
                .headers
                .iter()
                .filter_map(|header| header.location.modseq)
                .max()
        }),
        // A completed full snapshot clears any resumable initial-sync checkpoint.
        partial_initial_uid: None,
        updated_at,
    }
}

/// Upsert-only chunk batch for a resumable INITIAL full sync (B4).
///
/// Carries this chunk's projected messages + IMAP locations and the advancing
/// per-mailbox checkpoint (`state`). It NEVER sets `replace_all_messages` and
/// carries no deletions: a mid-sync checkpoint commits UPSERTS only and must not
/// drive prune-by-absence (the DS1 mail-loss invariant) — the local set is not
/// yet the complete remote set. Mailboxes are emitted once, separately, via
/// [`imap_mailbox_sync_batch`], so this batch carries none.
///
/// When `state.partial_initial_uid` is `Some`, the snapshot is still in progress
/// and a restart resumes from that UID; when it is `None` (and `highest_uid` is
/// set), this is the finalizing chunk that completes the snapshot.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
pub fn imap_initial_snapshot_chunk_batch(
    discovery: &DiscoveredImapAccount,
    headers: Vec<ImapMappedHeader>,
    state: ImapMailboxSyncState,
) -> SyncBatch {
    let ProjectedMessages {
        messages,
        locations,
        ..
    } = messages_and_locations_for_batch(discovery, headers);
    SyncBatch {
        mailboxes: Vec::new(),
        messages,
        imap_mailbox_states: vec![state],
        imap_message_locations: locations,
        deleted_imap_message_locations: Vec::new(),
        deleted_mailbox_ids: Vec::new(),
        deleted_message_ids: Vec::new(),
        absence_deleted_imap_message_locations: Vec::new(),
        absence_deleted_message_ids: Vec::new(),
        replace_all_mailboxes: false,
        replace_all_messages: false,
        cursors: Vec::new(),
    }
}

pub fn imap_mailbox_state_from_changed_since_snapshot(
    stored: &ImapMailboxSyncState,
    snapshot: &ImapChangedSinceSnapshot,
    updated_at: String,
) -> ImapMailboxSyncState {
    let mut state = ImapMailboxSyncState {
        mailbox_id: snapshot.selected.mailbox_id.clone(),
        mailbox_name: snapshot.selected.mailbox_name.clone(),
        uid_validity: snapshot.selected.uid_validity,
        highest_uid: stored.highest_uid,
        highest_modseq: stored.highest_modseq,
        partial_initial_uid: stored.partial_initial_uid,
        updated_at,
    };

    for header in &snapshot.headers {
        state.record_seen_uid(header.location.uid);
        if let Some(modseq) = header.location.modseq {
            state.record_highest_modseq(modseq);
        }
    }
    if let Some(highest_modseq) = snapshot.selected.highest_modseq {
        state.record_highest_modseq(highest_modseq);
    }

    state
}

#[cfg(test)]
mod tests;
