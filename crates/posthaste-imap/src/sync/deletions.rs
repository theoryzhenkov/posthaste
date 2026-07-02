use std::collections::{BTreeMap, BTreeSet};

use posthaste_domain_service::{
    ImapMessageLocation, ImapMessageLocationKey, ImapUid, ImapUidValidity, MailboxId, MessageId,
    MessageRecord,
};

pub(super) fn deleted_locations_missing_from_remote(
    local_locations: &[ImapMessageLocation],
    remote_locations: &BTreeSet<ImapMessageLocationKey>,
) -> Vec<ImapMessageLocationKey> {
    local_locations
        .iter()
        .map(ImapMessageLocation::key)
        .filter(|key| !remote_locations.contains(key))
        .collect()
}

pub(super) fn deleted_locations_matching_vanished_uids(
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

pub(super) fn deleted_locations_for_delta(
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

pub(super) fn deleted_message_ids_for_deleted_locations(
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

pub(super) fn preserve_delta_mailboxes_from_locations(
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
