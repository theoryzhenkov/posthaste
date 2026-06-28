---
scope: L2
summary: "The runtime near-node tier lags the client tier: it retires its backend-facing outbox unconditionally on receipt (not by absorption), so the flicker reappears one layer down the moment runtime↔backend goes remote. Plus latent remote-seam issues: the outbox overlay isn't account-scoped (cross-account fold + double-fold), phantom seq gaps on collapse, and a dead Conflict arm."
modified: 2026-06-28
reviewed: 2026-06-28
lifecycle: ephemeral
type: ISSUE
status: open
priority: medium
depends:
  - path: docs/eph/DESIGN-L2-mutation-notification
  - path: docs/replication/backend-link/L2
---

# Runtime near-node + remote-seam robustness

Co-located today these are benign, but the whole effort is making runtime↔backend
a switchable/remote transport; each item bites when the seam goes remote.

## A — Near-node retires unconditionally on receipt, not by absorption (MEDIUM) — RESOLVED 2026-06-28

`crates/posthaste-runtime/src/build.rs:586` fires `retire(&id)` immediately after
`run_message_mutation` returns, for both Ok and Err. Co-located this is safe
(backend applies before the receipt; the next recompute's base already carries
the effect). But when remote, the receipt can return **before** `message.updated`
propagates to the runtime's read replica → `retire` → recompute reads stale base
→ revert → re-apply on the later firehose ingest — structurally identical to the
`(stale base, no pending)` window the client-side fix eliminated. The two
near-node tiers are now inconsistent: client got absorption-gated retire, runtime
near-node kept unconditional retire.

**Fix:** retire the runtime outbox by absorption (reuse the `MessageReplica`
absorption test in `apply_outbox_overlay`/`set_base`), or at minimum gate the
retire on the down-channel having delivered the corresponding `message.updated`,
not on `forward.await`.

**Resolved (2026-06-28):** `RuntimeBackendOutbox` now wraps a `MessageReplica`
and its retirement is policy-driven by `drive_down_channel`. Co-located it drops
a confirmed op on receipt (`drop_pending`; the base already carries the effect —
`colocated-unchanged`). Remote it only `mark_confirmed`s on receipt and retires
by absorption when `run_backend_down_channel` applies the corresponding base
assertion (`apply_base` → `retire_absorbed`), so a receipt that outruns the
`message.updated` propagation no longer recomputes against a stale base. This
mirrors the client tier's absorption-gated retire. A rejection (`Failed` receipt)
still drops on receipt in either mode (the base never absorbs a rejection). Added
`Replica::drop_pending` to link-core for the unconditional co-located drop.

Provenance: four-reviewer Task 2 (MEDIUM-2).

## B — Outbox overlay not account-scoped; double-fold (LOW)

`crates/posthaste-runtime/src/near_node.rs:94-135` keys purely on
`row_message_id`, and the outbox is a single runtime-wide `RuntimeBackendOutbox`
(`build.rs:279`). Co-located it's empty so moot; remote/multi-account, a pending
op for account B could fold into account A's view on a message-id collision.
Also the same client op is folded at both the runtime tier and the client tier
(double-fold) — invisible only because the derivable assertions
(SetKeywords/Destroy/ApplyDiff) are idempotent.

**Fix:** scope the overlay by `account_scope` (filter `pending` to the session's
accounts); document the double-fold idempotency reliance as an invariant the
derivable-assertion set must preserve.

Provenance: four-reviewer Task 2 (LOW-6).

## C — Phantom seq gaps on collapse (LOW)

`crates/posthaste-runtime/src/sessions.rs:750`:
`mutation.notification_frame(next_seq(session))` evaluates `next_seq` eagerly, but
`notification_frame` returns `None` for non-terminal states — so a seq is
consumed with no frame emitted. The gaps make `subscribe_frames`'s
`after_seq == current_seq` fast-path miss (forcing redundant re-collapses) and
could trip a client gap-detector.

**Fix:** compute the notification first; allocate the seq only when a frame will
be emitted.

Provenance: four-reviewer Task 2 (LOW-5).

## D — Dead `Conflict`/`Queued`/`LocalApplied` arms (LOW, note)

`run_message_mutation` only ever settles `Confirmed`/`Failed`, so the
`MutationSettlementState::Conflict` arm in `notification()` (`sessions.rs:106`) is
unreachable — conflict is carried by `error.code`, so no information is lost
(design intent holds). Add a comment that the `Conflict` state arm is vestigial,
to stop a future reader wiring it.

Provenance: four-reviewer Task 2 (LOW-7).
