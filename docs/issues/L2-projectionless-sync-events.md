---
scope: L2
summary: "Sync-side message.updated events (IMAP expunge, location/membership removal, full delete, mailbox cleanup) carry no projection and no countDeltas, violating the rows+counts-on-one-stream invariant the store relies on. The store drops them AND the REST fallback is suppressed (skipStoreOwned) → row/count divergence until reload — the 'live updates drop until reload' class, reachable on every expunge/remote-delete."
modified: 2026-06-27
reviewed: 2026-06-27
lifecycle: ephemeral
type: ISSUE
status: open
priority: high
depends:
  - path: docs/eph/DESIGN-L2-client-link-reactive-store
  - path: docs/state/mail/L2
---

# Projection-less sync events break the rows+counts invariant

**Corroborated independently by two reviewers** (four-reviewer Task 2 HIGH-1 +
Task 3 HIGH-1). The strongest user-impacting correctness gap besides the flicker.

The reactive store assumes "2c attaches the projection (and countDeltas) to every
non-destroy `message.updated`." That holds on the **command** paths
(`crates/posthaste-store/src/mutations/commands.rs` `set_keywords_tx`/
`replace_mailboxes_tx`/`destroy_message_tx` all attach `payload["countDeltas"]` +
`projection`), but is **false on the sync side**:

- `crates/posthaste-store/src/mutations/projection_tracking.rs:188-210`
  (`delete_imap_message_location_and_track_projection_inputs`) — an IMAP expunge
  / membership-location removal emits `{messageId, changes:{mailboxes:true},
  mailboxIds, removedMailboxId}` with **no projection and no countDeltas**.
- `crates/posthaste-store/src/mutations/sync_batch.rs:95-101` — the full-delete
  `{messageId, deleted:true}` event, a row drop with **no countDeltas**.
- `crates/posthaste-store/src/mutations/mailbox_cleanup.rs:94-104` — mailbox-
  deletion cleanup, same bare shape.

**Effect when the store is active:** the adapter drops the event
(`apps/web/src/runtime/replica/entityStoreAdapter.ts:387`,
`if (!deleted && !projection) return`) **and** the REST fallback is suppressed
(`apps/web/src/domain-cache/handlers.ts:120`, `skipStoreOwned =
isEntityStoreAdapterActive()` → `invalidateMessageListReadModels` skipped). So
the affected row moves/drops per `changes.mailboxes` while `mailbox[id].count`
stays stale until a full resync/reload — a visible count/row divergence on every
IMAP expunge and remote delete. This is the "live updates drop until reload"
failure class.

## Fix

Prefer (a): make these Rust emitters attach `projection` + `countDeltas` like the
command path. The atomicity is already correct (event_log insert + trigger-
maintained counts commit in the same `write_transaction` tx); only the post-
mutation *read* is missing — compute `mailbox_counts_json_tx(tx, account_id,
affected)` (affected = removed mailbox for the location path; previous mailboxes
for the full delete) and set `payload["countDeltas"]`, mirroring
`destroy_message_tx`.

Fallback (b): in `ingestMessageEvent`, when a `changes.mailboxes`/`removedMailboxId`
event arrives without a projection, do **not** swallow the corresponding REST
invalidation (don't skip-store-owned for projection-less updates).

Add a test pinning a projection-less membership/expunge event through the adapter
(rows + counts both update).

## Provenance

Four-reviewer Task 2 (HIGH-1) + Task 3 (HIGH-1), corroborated. Relates to the
durable-clear-without-trace hazard in [[L2-outbox-op-lifecycle]] D (a dropped
absorbing event can leak an in-memory op).
