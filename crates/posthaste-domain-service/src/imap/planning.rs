use super::*;

/// Select the strongest correctness-preserving sync mode available for one mailbox.
///
/// QRESYNC and CONDSTORE are only usable when both the server advertises support
/// and the local store has a previous MODSEQ. Without MODSEQ, UID watermarks can
/// reconcile additions and expunges but cannot prove flag-only changes, so the
/// driver must refresh the mailbox metadata snapshot.
///
/// @spec docs/L0-providers#imap-delta-fallback
pub fn plan_imap_mailbox_sync(
    capabilities: &ImapCapabilities,
    provider: &ProviderProfile,
    stored: Option<&ImapMailboxSyncState>,
    selected: &ImapSelectedMailbox,
) -> ImapMailboxSyncPlan {
    let Some(stored) = stored else {
        return ImapMailboxSyncPlan::FullSnapshot {
            reason: ImapFullSyncReason::InitialSync,
        };
    };

    if !stored.is_valid_for(selected.uid_validity) {
        return ImapMailboxSyncPlan::FullSnapshot {
            reason: ImapFullSyncReason::UidValidityChanged,
        };
    }

    // An interrupted initial sync left a durable partial-sync checkpoint (B4).
    // UIDVALIDITY still matches (checked above), so the committed prefix is
    // sound: resume the snapshot from the checkpoint instead of taking any delta
    // path (a delta would trust the partial watermark and could prune the
    // not-yet-fetched tail). The executor reads `stored.partial_initial_uid` for
    // the resume point.
    if stored.partial_initial_uid.is_some() {
        return ImapMailboxSyncPlan::FullSnapshot {
            reason: ImapFullSyncReason::ResumeInitialSync,
        };
    }

    if let Some(reason) = provider.imap().required_full_sync_reason() {
        return ImapMailboxSyncPlan::FullSnapshot { reason };
    }

    if let (Some(since_modseq), Some(_)) = (stored.highest_modseq, selected.highest_modseq) {
        if capabilities.supports_qresync() && capabilities.supports_enable() {
            return ImapMailboxSyncPlan::QresyncDelta {
                uid_validity: selected.uid_validity,
                since_modseq,
                after_uid: stored.highest_uid,
            };
        }

        if capabilities.supports_condstore() {
            return ImapMailboxSyncPlan::CondstoreDelta {
                since_modseq,
                after_uid: stored.highest_uid,
            };
        }
    }

    if stored.highest_uid.is_some() {
        return ImapMailboxSyncPlan::FullSnapshot {
            reason: ImapFullSyncReason::FlagDeltaUnavailable,
        };
    }

    ImapMailboxSyncPlan::FullSnapshot {
        reason: ImapFullSyncReason::MissingUidWatermark,
    }
}

/// Select the safest available IMAP move strategy.
///
/// UIDPLUS lets the server report the destination UID after move/copy. Without
/// it, the command can still succeed, but local location state must be repaired
/// by a mailbox resync.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
pub fn plan_imap_move(capabilities: &ImapCapabilities) -> ImapMoveStrategy {
    if capabilities.supports_move() && capabilities.supports_uidplus() {
        ImapMoveStrategy::UidMoveWithCopyUid
    } else if capabilities.supports_move() {
        ImapMoveStrategy::UidMoveThenResync
    } else {
        ImapMoveStrategy::CopyDeleteThenResync
    }
}
