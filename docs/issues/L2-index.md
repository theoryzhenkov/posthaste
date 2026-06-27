---
scope: L2
summary: "Index of open ISSUE files for the client-link reactive store + mutation.notification work — findings preserved across the design/flicker-diagnosis, four-reviewer codebase-quality, and flicker-investigation rounds (2026-06-27)."
modified: 2026-06-27
reviewed: 2026-06-27
lifecycle: ephemeral
type: ISSUE
status: open
dependents:
  - path: docs/issues/L2-sugar-role-mutations
  - path: docs/issues/L2-single-source-view-membership
  - path: docs/issues/L2-reserve-clobbers-optimism
  - path: docs/issues/L2-outbox-op-lifecycle
  - path: docs/issues/L2-projectionless-sync-events
  - path: docs/issues/L2-engine-absorption-footguns
  - path: docs/issues/L2-runtime-nearnode-remote-seam
  - path: docs/issues/L2-adapter-reproject-all
  - path: docs/issues/L2-test-fakehandle-drift
  - path: docs/issues/L2-legacy-leftover-structures
  - path: docs/issues/L2-store-correctness-grabbag
---

# Open issues — reactive store + mutation.notification

Findings from three review rounds on the `views-stability` work (the client-link
reactive entity store 2a–2f + the mutation.notification / flicker-fix effort),
preserved so they aren't lost. None are committed-as-fixed; each file states its
own status, severity, location, mechanism, and proposed fix.

## By priority

| Priority | Issue | One-line |
|---|---|---|
| ~~HIGH~~ **DONE** | [[L2-reserve-clobbers-optimism]] | **The user-reported on-mutation flicker — FIXED, shipped in `.21`** (independently verified). A: `set_view_rows` reconcile + version-gated retire + role-move optimism; B: per-message version guard + confirmed-gated retire. Deeper follow-up: [[L2-single-source-view-membership]]. |
| **HIGH** | [[L2-outbox-op-lifecycle]] | Op leaks forever on authoritative delete; cancelled dispatch leaks as `Accepted`; `Rejected` evicted → permanent ghost; durable outbox cleared on confirm even when not retired. |
| **HIGH** | [[L2-projectionless-sync-events]] | Sync/expunge/membership `message.updated` carry no projection+counts → store drops + REST skipped → row/count divergence until reload. |
| ~~HIGH~~ **DONE** | [[L2-test-fakehandle-drift]] | **Retargeted onto the real wasm handle** — the 250-line drifting `FakeHandle` deleted; all 8 adapter tests now real-verified (false-green class retired). |
| LOW (was MED) | [[L2-engine-absorption-footguns]] | **Mostly resolved**: dead base-update methods gone; `settle(Confirmed)` now confirmed-gated; engine gained `mark_confirmed`/`retire_absorbed_if`. Residual: version-gating lives in link-replica not the engine — lift it up if a 2nd convergence consumer appears. |
| MEDIUM | [[L2-runtime-nearnode-remote-seam]] | Runtime near-node retires unconditionally on receipt (flicker when remote); overlay not account-scoped; phantom seq gaps; dead Conflict arm. |
| MEDIUM | [[L2-adapter-reproject-all]] | Adapter re-projects every open view every drain (O(views×rows)); needs a message→views reverse index. |
| MEDIUM | [[L2-legacy-leftover-structures]] | **Mostly resolved**: the dual-path mail-list query (loaded gun) + `fetchMessagesForView` + `runtimeMailListViewsEnabled` flag retired — the list is single-source via `useRuntimeMailListView`; dead `useDomainEventRefresh` gone. **Still open:** the ungated invalidation storm (sidebar/nav/detail) during sync; rejection has no UI. |
| DONE | [[L2-sugar-role-mutations]] | `archive`/`trash`/`restoreToInbox` were 1:1 aliases of `moveToRole` (no extra authority semantics); collapsed into `moveToRole`. `destroy` kept (genuine permanent deletion). |
| MEDIUM | [[L2-single-source-view-membership]] | The dual-source membership smell: retire the runtime's redundant incremental-membership re-serve so the firehose is the single source of truth for evaluable views (perf + one channel). The deeper cleanup the `set_view_rows` reconcile is a stepping-stone toward. |
| MIXED | [[L2-store-correctness-grabbag]] | `in_range` ignores sort direction (HIGH); timestamp lexicographic sort; no base GC; `writeMailboxCount` unknown-account; unguarded async; nits. |

## Cross-cutting themes

- **Reconcile, don't clobber** — the reused runtime structures (view re-serve,
  legacy invalidations) are trusted blindly by the store. Most of the active bugs
  trace here: [[L2-reserve-clobbers-optimism]], [[L2-legacy-leftover-structures]].
  The end-state is one source of truth: [[L2-single-source-view-membership]].
- **The absorption invariant is enforced in only one layer** — the store. The
  engine and the runtime near-node still expose/use the pre-fix retire:
  [[L2-engine-absorption-footguns]], [[L2-runtime-nearnode-remote-seam]].
- **One stream must carry rows *and* counts** — violated on the sync side:
  [[L2-projectionless-sync-events]] (compounds the durable-leak in
  [[L2-outbox-op-lifecycle]] D).
- **Test fidelity gates trust** — the fake-engine drift
  ([[L2-test-fakehandle-drift]]) is *why* several of these shipped uncaught
  (notably the delete-leak, which the fake accidentally "fixes").

## Suggested sequencing

1. ~~[[L2-reserve-clobbers-optimism]] — the live flicker.~~ **DONE (shipped `.21`).**
2. ~~[[L2-test-fakehandle-drift]] — retarget onto real wasm.~~ **DONE** — the adapter test now drives the real engine; false-green class retired.
3. **[[L2-outbox-op-lifecycle]] A + [[L2-projectionless-sync-events]] — corroborated correctness bugs (now verifiable against the real engine). ← NEXT.**
4. ~~[[L2-engine-absorption-footguns]]~~ — mostly resolved; residual is LOW documentation.
5. Remainder (perf, remote-seam, legacy cleanup, grab-bag, [[L2-single-source-view-membership]]) as the seam/scale demands.

## Resolved this round (2026-06-27)

- [[L2-reserve-clobbers-optimism]] — the on-mutation flicker (A+B), shipped `.21`.
- [[L2-sugar-role-mutations]] — archive/trash/restoreToInbox collapsed into `moveToRole`.
- [[L2-test-fakehandle-drift]] — adapter test retargeted onto the real wasm engine; `FakeHandle` deleted.
- [[L2-legacy-leftover-structures]] A+C — the legacy mail-list REST fork + flag retired (single-source via `useRuntimeMailListView`); B (invalidation storm) remains.
- [[L2-engine-absorption-footguns]] — mostly resolved (dead methods gone; confirmed-gating in the engine).
