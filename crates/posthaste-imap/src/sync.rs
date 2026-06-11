use std::collections::{BTreeMap, BTreeSet};

use posthaste_domain::{
    AccountId, ImapMailboxSyncState, ImapMessageLocation, ImapMessageLocationKey, ImapUid,
    ImapUidValidity, MailboxId, MailboxRecord, MessageId, MessageRecord, SyncBatch, SyncCursor,
    SyncObject,
};

use crate::{
    message::ImapMailboxMembershipSource, provider::ImapAdapterProviderProfile,
    DiscoveredImapAccount, ImapChangedSinceSnapshot, ImapMailboxHeaderSnapshot, ImapMappedHeader,
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
        highest_modseq: snapshot
            .headers
            .iter()
            .filter_map(|header| header.location.modseq)
            .max(),
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

fn messages_and_locations_for_batch(
    discovery: &DiscoveredImapAccount,
    headers: Vec<ImapMappedHeader>,
) -> ProjectedMessages {
    messages_and_locations_from_headers(project_imap_headers(discovery, headers))
}

struct ProjectedMessages {
    messages: Vec<MessageRecord>,
    locations: Vec<ImapMessageLocation>,
    provider_absent_mailbox_ids_by_message: BTreeMap<MessageId, BTreeSet<MailboxId>>,
}

fn messages_and_locations_from_headers(headers: Vec<ImapMappedHeader>) -> ProjectedMessages {
    let mut messages_by_id = BTreeMap::<MessageId, MessageRecord>::new();
    let mut locations = Vec::with_capacity(headers.len());
    let mut provider_absent_mailbox_ids_by_message =
        BTreeMap::<MessageId, BTreeSet<MailboxId>>::new();

    for header in headers {
        if header.mailbox_membership_source == ImapMailboxMembershipSource::ProviderLabels {
            provider_absent_mailbox_ids_by_message
                .entry(header.message.id.clone())
                .or_default()
                .extend(header.provider_absent_mailbox_ids.iter().cloned());
        }
        messages_by_id
            .entry(header.message.id.clone())
            .or_insert(header.message);
        locations.push(header.location);
    }

    ProjectedMessages {
        messages: messages_by_id.into_values().collect(),
        locations,
        provider_absent_mailbox_ids_by_message,
    }
}

fn project_imap_headers(
    discovery: &DiscoveredImapAccount,
    headers: Vec<ImapMappedHeader>,
) -> Vec<ImapMappedHeader> {
    ImapAdapterProviderProfile::from_discovery(discovery).project_headers(headers)
}

fn deleted_locations_missing_from_remote(
    local_locations: &[ImapMessageLocation],
    remote_locations: &BTreeSet<ImapMessageLocationKey>,
) -> Vec<ImapMessageLocationKey> {
    local_locations
        .iter()
        .map(ImapMessageLocation::key)
        .filter(|key| !remote_locations.contains(key))
        .collect()
}

fn deleted_locations_matching_vanished_uids(
    local_locations: &[ImapMessageLocation],
    vanished_locations: &BTreeSet<(MailboxId, ImapUidValidity, ImapUid)>,
) -> Vec<ImapMessageLocationKey> {
    local_locations
        .iter()
        .map(ImapMessageLocation::key)
        .filter(|key| {
            vanished_locations.contains(&(key.mailbox_id.clone(), key.uid_validity, key.uid))
        })
        .collect()
}

fn deleted_locations_for_delta(
    local_locations: &[ImapMessageLocation],
    base_deleted_locations: Vec<ImapMessageLocationKey>,
    provider_absent_mailbox_ids_by_message: &BTreeMap<MessageId, BTreeSet<MailboxId>>,
) -> Vec<ImapMessageLocationKey> {
    let mut deleted_locations = base_deleted_locations;
    deleted_locations.extend(
        local_locations
            .iter()
            .filter(|location| {
                provider_absent_mailbox_ids_by_message
                    .get(&location.message_id)
                    .is_some_and(|mailbox_ids| mailbox_ids.contains(&location.mailbox_id))
            })
            .map(ImapMessageLocation::key),
    );
    deduplicate_location_keys(deleted_locations)
}

fn deleted_message_ids_for_deleted_locations(
    local_locations: &[ImapMessageLocation],
    deleted_locations: &[ImapMessageLocationKey],
    new_locations: &[ImapMessageLocation],
) -> Vec<MessageId> {
    let deleted_keys = deleted_locations.iter().cloned().collect::<BTreeSet<_>>();
    let mut remaining_location_counts = BTreeMap::<MessageId, usize>::new();

    for location in local_locations {
        if deleted_keys.contains(&location.key()) {
            continue;
        }
        *remaining_location_counts
            .entry(location.message_id.clone())
            .or_default() += 1;
    }
    for location in new_locations {
        *remaining_location_counts
            .entry(location.message_id.clone())
            .or_default() += 1;
    }

    deduplicate_message_ids(
        deleted_locations
            .iter()
            .filter(|key| {
                remaining_location_counts
                    .get(&key.message_id)
                    .copied()
                    .unwrap_or(0)
                    == 0
            })
            .map(|key| key.message_id.clone())
            .collect(),
    )
}

fn preserve_delta_mailboxes_from_locations(
    messages: &mut [MessageRecord],
    local_locations: &[ImapMessageLocation],
    deleted_locations: &[ImapMessageLocationKey],
    new_locations: &[ImapMessageLocation],
    provider_absent_mailbox_ids_by_message: &BTreeMap<MessageId, BTreeSet<MailboxId>>,
) {
    let deleted_keys = deleted_locations.iter().cloned().collect::<BTreeSet<_>>();
    let mut mailbox_ids_by_message = BTreeMap::<MessageId, BTreeSet<MailboxId>>::new();

    for location in local_locations {
        if deleted_keys.contains(&location.key()) {
            continue;
        }
        if provider_absent_mailbox_ids_by_message
            .get(&location.message_id)
            .is_some_and(|mailbox_ids| mailbox_ids.contains(&location.mailbox_id))
        {
            continue;
        }
        mailbox_ids_by_message
            .entry(location.message_id.clone())
            .or_default()
            .insert(location.mailbox_id.clone());
    }
    for location in new_locations {
        mailbox_ids_by_message
            .entry(location.message_id.clone())
            .or_default()
            .insert(location.mailbox_id.clone());
    }

    for message in messages {
        if let Some(mailbox_ids) = mailbox_ids_by_message.remove(&message.id) {
            message.mailbox_ids = mailbox_ids.into_iter().collect();
        }
    }
}

fn deduplicate_location_keys(keys: Vec<ImapMessageLocationKey>) -> Vec<ImapMessageLocationKey> {
    let mut seen = BTreeSet::new();
    let mut deduplicated = Vec::with_capacity(keys.len());
    for key in keys {
        if seen.insert(key.clone()) {
            deduplicated.push(key);
        }
    }
    deduplicated
}

fn deduplicate_message_ids(mut message_ids: Vec<MessageId>) -> Vec<MessageId> {
    message_ids.sort();
    message_ids.dedup();
    message_ids
}

fn mailbox_cursor_state(mailboxes: &[MailboxRecord]) -> String {
    let mut fingerprint = String::new();
    for mailbox in mailboxes {
        fingerprint.push_str(mailbox.id.as_str());
        fingerprint.push('\0');
        fingerprint.push_str(&mailbox.name);
        fingerprint.push('\0');
        fingerprint.push_str(mailbox.role.as_deref().unwrap_or(""));
        fingerprint.push('\0');
    }
    format!("imap-mailboxes:{}", hex::encode(fingerprint.as_bytes()))
}

fn message_cursor_state(messages: &[MessageRecord], locations: &[ImapMessageLocation]) -> String {
    let mut fingerprint = String::new();
    for message in messages {
        fingerprint.push_str(message.id.as_str());
        fingerprint.push('\0');
    }
    for location in locations {
        fingerprint.push_str(location.message_id.as_str());
        fingerprint.push('\0');
        fingerprint.push_str(location.mailbox_id.as_str());
        fingerprint.push('\0');
        fingerprint.push_str(&location.uid_validity.0.to_string());
        fingerprint.push('\0');
        fingerprint.push_str(&location.uid.0.to_string());
        fingerprint.push('\0');
    }
    format!("imap-messages:{}", hex::encode(fingerprint.as_bytes()))
}

#[cfg(test)]
mod tests;
