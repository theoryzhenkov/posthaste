use std::collections::BTreeSet;

use posthaste_domain_model::{ImapMailboxSyncState, ImapMessageLocation, ImapUid, ImapUidValidity, MailboxRecord, SyncBatch, SyncCursor, SyncObject};
use posthaste_domain_model::{AccountId, MailboxId};

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
    preserve_delta_mailboxes_from_locations,
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

pub fn imap_condstore_delta_sync_batch(
    account_id: &AccountId,
    discovery: DiscoveredImapAccount,
    headers: Vec<ImapMappedHeader>,
    mailbox_states: Vec<ImapMailboxSyncState>,
    local_locations: Vec<ImapMessageLocation>,
    vanished_uids: Vec<(MailboxId, ImapUidValidity, ImapUid)>,
    updated_at: String,
) -> SyncBatch {
    let ProjectedMessages {
        mut messages,
        locations,
        provider_absent_mailbox_ids_by_message,
    } = messages_and_locations_for_batch(&discovery, headers);
    let mut batch = imap_mailbox_sync_batch(account_id, discovery, updated_at.clone());
    let vanished_locations = vanished_uids.into_iter().collect::<BTreeSet<_>>();
    let deleted_imap_message_locations = deleted_locations_for_delta(
        &local_locations,
        deleted_locations_matching_vanished_uids(&local_locations, &vanished_locations),
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
        updated_at,
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
