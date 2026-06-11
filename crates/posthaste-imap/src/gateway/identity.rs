use super::*;

/// Project a local message location to its (mailbox, uid-validity, uid) identity tuple.
pub(crate) fn location_identity(
    location: &ImapMessageLocation,
) -> (MailboxId, ImapUidValidity, ImapUid) {
    (
        location.mailbox_id.clone(),
        location.uid_validity,
        location.uid,
    )
}

pub(crate) fn missing_location_identities(
    local_locations: &[ImapMessageLocation],
    remote_headers: &[ImapMappedHeader],
) -> Vec<(MailboxId, ImapUidValidity, ImapUid)> {
    let remote_locations = remote_headers
        .iter()
        .map(|header| {
            (
                header.location.mailbox_id.clone(),
                header.location.uid_validity.0,
                header.location.uid,
            )
        })
        .collect::<std::collections::BTreeSet<_>>();

    local_locations
        .iter()
        .filter(|location| {
            !remote_locations.contains(&(
                location.mailbox_id.clone(),
                location.uid_validity.0,
                location.uid,
            ))
        })
        .map(location_identity)
        .collect()
}

pub(crate) fn missing_location_identities_from_uids(
    local_locations: &[ImapMessageLocation],
    remote_uids: &[ImapUid],
) -> Vec<(MailboxId, ImapUidValidity, ImapUid)> {
    let remote_uids = remote_uids
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();

    local_locations
        .iter()
        .filter(|location| !remote_uids.contains(&location.uid))
        .map(location_identity)
        .collect()
}

pub(crate) fn imap_sync_plan_name(plan: &ImapMailboxSyncPlan) -> &'static str {
    match plan {
        ImapMailboxSyncPlan::FullSnapshot { .. } => "full_snapshot",
        ImapMailboxSyncPlan::FetchNewByUid { .. } => "fetch_new_by_uid",
        ImapMailboxSyncPlan::CondstoreDelta { .. } => "condstore_delta",
        ImapMailboxSyncPlan::QresyncDelta { .. } => "qresync_delta",
    }
}

pub(crate) fn planned_imap_sync_plan_name(plan: &PlannedImapMailboxSync) -> &'static str {
    match plan {
        PlannedImapMailboxSync::SkipUnchanged => "skip_unchanged",
        PlannedImapMailboxSync::Sync(plan) => imap_sync_plan_name(plan),
    }
}
