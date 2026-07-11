//! Property-based coverage of the mail-safety prune/floor-guard invariants
//! (`MAX_ABSENCE_PRUNE_FRACTION`, the empty-remote refusal, and the
//! `force_full_prune` bypass). (NS1: the M35 protected-id exemption is gone —
//! optimism lives in the overlay plane, which the base prune cannot touch.)
//!
//! The fixed-example snapshot tests (`message_snapshots`, `mailbox_snapshots`)
//! prove specific points; these generalize across random store sizes and prune
//! fractions so the boundary cases the examples miss — exactly-at-the-floor
//! deletions, empty-but-`Ok` remote sets — cannot silently regress. This is the guardrail the audit named for the DS1 /
//! DP-C2/C3/C4 mail-loss class ("DS1 was found by luck; zero proptest coverage").
//!
//! Two paths are exercised:
//!   * the real `apply_sync_batch` snapshot entry (`replace_all_messages` /
//!     `replace_all_mailboxes`) — the wiring that actually shipped the bugs;
//!   * a direct call into `prune_messages_absent_from_remote_tx` for the
//!     `force_full_prune` invariant, which cannot be driven through the
//!     public API.

use std::collections::BTreeSet;

use proptest::prelude::*;

use super::*;

/// The floor: a prune-by-absence pass may never delete strictly MORE than this
/// fraction of the local store. Mirrors `sync_batch::MAX_ABSENCE_PRUNE_FRACTION`
/// (kept as a local literal so a change to the guard must be a deliberate change
/// to this oracle too).
const MAX_ABSENCE_PRUNE_FRACTION: f64 = 0.5;

/// Whether a prune-by-absence pass over `local_count` rows, of which `would_prune`
/// are absent-and-prunable, must be REFUSED by the floor/empty guard.
fn guard_refuses(local_count: usize, remote_is_empty: bool, would_prune: usize) -> bool {
    if local_count == 0 {
        return false; // nothing to protect; the guard short-circuits.
    }
    remote_is_empty || (would_prune as f64) > (local_count as f64) * MAX_ABSENCE_PRUNE_FRACTION
}

/// `(n, keep)`: a store of `n` rows and, for each, whether it is present in the
/// remote snapshot. `keep[i] == false` means row `i` is absent → a prune candidate.
fn snapshot_case(max: usize) -> impl Strategy<Value = (usize, Vec<bool>)> {
    (1..=max).prop_flat_map(|n| (Just(n), prop::collection::vec(any::<bool>(), n)))
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 96, ..ProptestConfig::default() })]

    /// A `replace_all_messages` snapshot must NEVER delete more than the floor,
    /// must preserve everything on an empty/drastic-shrink remote set, and under
    /// the floor must delete exactly the absent rows. (DS1 / DP-C2.)
    #[test]
    fn message_snapshot_prune_respects_floor_and_empty_guard((n, keep) in snapshot_case(16)) {
        let root = temp_root();
        let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data")).unwrap();
        let account = AccountId::from("primary");
        setup_source(&store, &account, "Primary").unwrap();

        let ids: Vec<String> = (0..n).map(|i| format!("msg-{i:03}")).collect();
        let seed = ids
            .iter()
            .map(|id| sample_message(id, "inbox", Some("mime")))
            .collect();
        seed_messages(&store, &account, seed, "seed-state").unwrap();

        let remote_present: BTreeSet<String> = ids
            .iter()
            .zip(&keep)
            .filter(|(_, &k)| k)
            .map(|(id, _)| id.clone())
            .collect();
        let snapshot_messages = remote_present
            .iter()
            .map(|id| sample_message(id, "inbox", Some("mime")))
            .collect();

        store
            .apply_sync_batch(
                &account,
                &SyncBatch {
                    mailboxes: Vec::new(),
                    messages: snapshot_messages,
                    replace_all_messages: true,
                    cursors: vec![message_cursor("snap", "2026-03-31T11:00:00Z")],
                    ..SyncBatch::default()
                },
            )
            .unwrap();

        let would_prune = n - remote_present.len();
        let refused = guard_refuses(n, remote_present.is_empty(), would_prune);

        let survivors: BTreeSet<String> = store
            .list_messages(&account, None)
            .unwrap()
            .into_iter()
            .map(|m| m.id.to_string())
            .collect();

        if refused {
            prop_assert_eq!(
                survivors.len(),
                n,
                "guard refused (would_prune={} of {}) but did not preserve all local mail",
                would_prune,
                n
            );
        } else {
            prop_assert_eq!(
                &survivors,
                &remote_present,
                "under the floor, exactly the remote-present messages survive"
            );
        }

        // The invariant that would have caught DS1, asserted unconditionally: a
        // prune-by-absence snapshot never deletes more than half the store.
        let deleted = n - survivors.len();
        prop_assert!(
            (deleted as f64) <= (n as f64) * MAX_ABSENCE_PRUNE_FRACTION,
            "deleted {} of {} — exceeds the {:.0}% floor",
            deleted,
            n,
            MAX_ABSENCE_PRUNE_FRACTION * 100.0
        );
    }

    /// The same guard on the mailbox snapshot path (DP-C3): a capped/empty
    /// `Mailbox/query` must never cascade-delete the local mailbox set.
    #[test]
    fn mailbox_snapshot_prune_respects_floor_and_empty_guard((n, keep) in snapshot_case(12)) {
        let root = temp_root();
        let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data")).unwrap();
        let account = AccountId::from("primary");
        setup_source(&store, &account, "Primary").unwrap();

        let ids: Vec<String> = (0..n).map(|i| format!("mbx-{i:03}")).collect();
        store
            .apply_sync_batch(
                &account,
                &SyncBatch {
                    mailboxes: ids.iter().map(|id| mailbox_record(id)).collect(),
                    replace_all_mailboxes: true,
                    ..SyncBatch::default()
                },
            )
            .unwrap();

        let remote_present: BTreeSet<String> = ids
            .iter()
            .zip(&keep)
            .filter(|(_, &k)| k)
            .map(|(id, _)| id.clone())
            .collect();

        store
            .apply_sync_batch(
                &account,
                &SyncBatch {
                    mailboxes: remote_present.iter().map(|id| mailbox_record(id)).collect(),
                    replace_all_mailboxes: true,
                    ..SyncBatch::default()
                },
            )
            .unwrap();

        let would_prune = n - remote_present.len();
        let refused = guard_refuses(n, remote_present.is_empty(), would_prune);

        let survivors: BTreeSet<String> = store
            .list_mailboxes(&account)
            .unwrap()
            .into_iter()
            .map(|m| m.id.to_string())
            .collect();

        if refused {
            prop_assert_eq!(
                survivors.len(),
                n,
                "mailbox guard refused (would_prune={} of {}) but did not preserve all mailboxes",
                would_prune,
                n
            );
        } else {
            prop_assert_eq!(
                &survivors,
                &remote_present,
                "under the floor, exactly the remote-present mailboxes survive"
            );
        }
    }

    /// Direct-call invariant: `force_full_prune` bypasses the floor entirely
    /// (an explicit full-resync may shrink the store past 50%).
    #[test]
    fn force_full_prune_bypasses_floor(
        (n, keep) in snapshot_case(16),
    ) {
        let root = temp_root();
        let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data")).unwrap();
        let account = AccountId::from("primary");
        setup_source(&store, &account, "Primary").unwrap();

        let ids: Vec<String> = (0..n).map(|i| format!("msg-{i:03}")).collect();
        let seed = ids
            .iter()
            .map(|id| sample_message(id, "inbox", Some("mime")))
            .collect();
        seed_messages(&store, &account, seed, "seed-state").unwrap();

        let remote_present: std::collections::HashSet<&String> = ids
            .iter()
            .zip(&keep)
            .filter(|(_, &k)| k)
            .map(|(id, _)| id)
            .collect();
        let remote_ids: BTreeSet<MessageId> =
            remote_present.iter().map(|id| MessageId::from(id.as_str())).collect();

        store
            .write_transaction(|tx| -> Result<(), StoreError> {
                let mut events =
                    crate::projections::EventRecorder::with_capacity(tx, &account, 64)?;
                let mut affected = crate::mutations::types::ProjectionInputs::default();
                crate::mutations::sync_batch::prune_messages_absent_from_remote_tx(
                    tx,
                    &account,
                    &remote_ids,
                    true, // force_full_prune
                    &mut affected,
                    &mut events,
                )
            })
            .unwrap();

        let survivors: BTreeSet<String> = store
            .list_messages(&account, None)
            .unwrap()
            .into_iter()
            .map(|m| m.id.to_string())
            .collect();

        // Survivor set == remote-present; everything else is gone, regardless
        // of how large the deleted fraction is.
        let expected: BTreeSet<String> = ids
            .iter()
            .filter(|id| remote_present.contains(*id))
            .cloned()
            .collect();
        prop_assert_eq!(&survivors, &expected, "force prune keeps exactly the remote-present set");
    }
}

fn mailbox_record(id: &str) -> posthaste_domain_model::MailboxRecord {
    posthaste_domain_model::MailboxRecord {
        id: MailboxId::from(id),
        name: id.to_string(),
        role: None,
        unread_emails: 0,
        total_emails: 0,
    }
}
