---
scope: L2
summary: "Holes in the optimistic-op retirement + durable-outbox lifecycle: ops leak forever on authoritative message removal, cancelled dispatches leak as never-pruned Accepted, Rejected verdicts can be evicted before reconnect (permanent ghost), and the durable outbox is cleared on confirm even when the engine did not actually retire."
modified: 2026-06-27
reviewed: 2026-06-27
lifecycle: ephemeral
type: ISSUE
status: open
priority: high
depends:
  - path: docs/eph/DESIGN-L2-mutation-notification
  - path: docs/eph/DESIGN-L2-client-link-reactive-store
---

# Optimistic-op lifecycle leaks

The absorption-gated retire is correct on the happy path, but several lifecycle
edges leave optimistic ops (in-engine and/or durable) stuck forever or cleared
too early.

## A — Op leaks forever on authoritative message removal (CRITICAL, corroborated) — **RESOLVED**

**Fix landed:** `Replica::remove_pending(&key)` added to the engine
(`posthaste-link-core/src/convergence.rs`) — drops every pending op on a key
(both confirmed and unconfirmed) + clears their confirmed markers; the store
calls it in `apply_message`'s `deleted` branch, after `remove_base`. Scoped to
`deleted=true` (a never-ingested entity is not an authoritative removal; its
deferred pending must survive to fold on a later ingest). Regression test
`authoritative_delete_purges_pending_op` (real engine) — verified it fails without
the fix (op stuck pending) and passes with it; a late `settle(Confirmed)` on the
purged op is a no-op (settle finds no key → no retire), not a leak.

**Original finding (preserved).**

`retire_absorbed` early-returns `false` when the key has no base
(`crates/posthaste-link-core/src/convergence.rs:217`); the store's
`settle(Confirmed)` retires *only* via `retire_absorbed`
(`crates/posthaste-link-replica/src/entity_store.rs:339`); and
`apply_message(deleted=true)` does `remove_base` but **never purges pending**
(`entity_store.rs:364`). So any pending op (flag, move, destroy) on a message
that is then authoritatively deleted (a rule, another client, an expunge) stays
pending **forever**: `has_pending()` is stuck `true` (it drives optimistic-UI
affordances — a spinner/affordance that never clears) and the outbox grows
unbounded on a delete-heavy workload. No test covers `settle(Confirmed)` on a
removed message; the engine's `destroy_then_authoritative_removal` test uses the
*unconditional* `Replica::settle` the store no longer calls on the confirmed
path, masking the gap. (Tellingly, the TS `FakeHandle` *does* purge pending on
delete — the fake does the right thing the real engine is missing, which is why
nothing caught it; see [[L2-test-fakehandle-drift]].)

**Fix:** purge pending ops for a key on authoritative removal — in
`apply_message`'s deleted branch, after `remove_base`, drop every pending op on
that key (e.g. `Replica::remove_pending(&key)`). Do **not** blanket-retire on
"absent base" inside `settle` — absent base also means *never-ingested* (the
deferred-pending path), which must still fold in on a later ingest.

Provenance: four-reviewer Task 1 (C1) + Task 4 (H3), corroborated.

## B — Cancelled dispatch leaks as never-pruned `Accepted` (MEDIUM) — **RESOLVED**

`accept_mutation` inserts into `latest_mutations`/`mutations_by_client_id`
(`crates/posthaste-runtime/src/sessions.rs:400`) but only `settle_mutation`
pushes to `settled_mutation_ids`, and `prune_settled_mutations` only drains that
deque (`sessions.rs:77`). If a dispatch future is cancelled (client disconnects
mid-`forward.await`), the mutation is stuck `Accepted` forever — pruned by
nothing (unbounded map growth) and, on reconnect, `collapse_session_frames`
emits no frame for it. The client's outbox op then waits on an absorption that
may never come.

**Fix:** bound the non-terminal set too (TTL or count cap on `latest_mutations`
independent of `settled_mutation_ids`), and either re-emit an in-flight signal
on reconnect or guarantee a terminal settle on cancellation (settle `Failed` in
a drop-guard around `run_message_mutation`).

**Resolved:** went with the terminal-settle-on-cancel option. A
`MutationCancelGuard` drop-guard in `run_message_mutation`
(`crates/posthaste-runtime/src/build.rs`) is armed after `accept_mutation` and
disarmed on each normal settle path (Confirmed/Failed). On cancel — the dispatch
future dropped mid-`forward.await` (client disconnect) — the guard's `Drop` calls
`settle_mutation(Failed)`, guaranteeing a terminal verdict and pruning. The
bounding-cap alternative was unnecessary: the guard eliminates the leak at the
source rather than capping it. Regression test
`cancelled_dispatch_guard_settles_failed_not_accepted` verifies the guard
settles `Failed` (fails-without/passes-with). The arm/disarm wiring in
`run_message_mutation` is 4 lines, reviewed alongside (a full cancel-spawn test
was blocked: `RuntimeHandle` has no in-crate constructor, and the testkit
dev-dep cycle creates a second `posthaste-runtime` instance so `pub(crate)`
doesn't cross — the guard-level test is the reliable seam).

Provenance: four-reviewer Task 2 (MEDIUM-3).

## C — `Rejected` verdicts evicted by the reconnect cap → permanent ghost (MEDIUM)

`prune_settled_mutations` evicts oldest terminal verdicts FIFO under a uniform
`MAX_LATEST_MUTATIONS = 100` (`sessions.rs:77`). `Confirmed` eviction is safe
(absorption retires the op from the re-served snapshot). But a `Rejected` op is
retired **only** by receiving its verdict — the base never absorbs it (a
rejection changes no state). If a rejection is followed by >100 settlements while
the client is disconnected, the `Rejected` frame is evicted before reconnect and
the client **never reverts**: a stuck optimistic row with no recovery path.

**Fix:** retain `Rejected` verdicts under a separate, larger (or unbounded-until-
acked) budget; never evict an un-replayed rejection. The uniform cap conflates
two outcomes with very different recoverability.

Provenance: four-reviewer Task 2 (MEDIUM-4).

## D — Durable outbox cleared on `confirmed` even when the engine did not retire (MEDIUM)

`settleAll` (`apps/web/src/runtime/replica/entityStoreAdapter.ts:408`) calls
`handle.settle(...)` then unconditionally `outbox.remove(clientMutationId)`. But
absorption-gated confirm leaves the op **pending in-engine** when the base hasn't
caught up — and the WASM `settle` returns only `.reverted`
(`crates/posthaste-link-wasm/src/entity_store.rs:163`), **dropping the `retired`
bool** at the boundary. So the host cannot tell a no-op confirm from a real
retire and clears the durable record either way. If the user reloads in the
confirm-before-base window, `openMailListView` rehydrates from `outbox.all()` and
the optimism is lost; and if the absorbing `message.updated` is ever dropped (see
[[L2-projectionless-sync-events]]), the in-memory op leaks pending with no
durable trace.

**Fix:** expose `retired` from the WASM `settle` (return both, or a small JSON
result); in `settleAll`, clear the durable record only when the op actually
retired (confirmed+retired, or failed) — otherwise keep it until a later
absorbing base update retires it.

Provenance: four-reviewer Task 3 (MEDIUM-3).
