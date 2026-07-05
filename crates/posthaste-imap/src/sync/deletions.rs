use std::collections::{BTreeMap, BTreeSet};

use posthaste_domain_model::{ImapMessageLocation, ImapMessageLocationKey, ImapUid, ImapUidValidity, MessageRecord};
use posthaste_domain_model::{MailboxId, MessageId};

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

/// Partition the message ids of a set of deleted locations into AUTHORITATIVE
/// (server-asserted VANISHED) and ABSENCE-derived (inferred from a possibly-
/// truncated `UID SEARCH UNDELETED` / header listing) buckets.
///
/// A message id is authoritative only when EVERY one of its removed local
/// locations is a VANISHED removal; if even one removal was absence-derived, the
/// conclusion "this message is gone" rests on an inference and the deletion is
/// routed through the store's DP-C4 floor guard. `authoritative_keys` and
/// `absence_keys` are disjoint location-key sets (a VANISHED key wins over an
/// absence key for the same location — see the builder). The input
/// `fully_gone_message_ids` is the deduped set of messages with no surviving
/// location (as computed by [`deleted_message_ids_for_deleted_locations`] over
/// the union of both key sets).
pub(super) fn partition_deleted_message_ids_by_origin(
    local_locations: &[ImapMessageLocation],
    fully_gone_message_ids: &[MessageId],
    authoritative_keys: &BTreeSet<ImapMessageLocationKey>,
    absence_keys: &BTreeSet<ImapMessageLocationKey>,
) -> (Vec<MessageId>, Vec<MessageId>) {
    let fully_gone = fully_gone_message_ids.iter().cloned().collect::<BTreeSet<_>>();
    let mut removed_keys_by_message = BTreeMap::<MessageId, Vec<ImapMessageLocationKey>>::new();
    for location in local_locations {
        let key = location.key();
        let is_removed = authoritative_keys.contains(&key) || absence_keys.contains(&key);
        if is_removed && fully_gone.contains(&location.message_id) {
            removed_keys_by_message
                .entry(location.message_id.clone())
                .or_default()
                .push(key);
        }
    }

    let mut authoritative = Vec::new();
    let mut absence = Vec::new();
    for message_id in &fully_gone {
        let all_authoritative = removed_keys_by_message
            .get(message_id)
            .map(|keys| keys.iter().all(|key| authoritative_keys.contains(key)))
            .unwrap_or(false);
        if all_authoritative {
            authoritative.push(message_id.clone());
        } else {
            absence.push(message_id.clone());
        }
    }
    (authoritative, absence)
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
