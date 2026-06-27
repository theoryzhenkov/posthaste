---
scope: L2
summary: "The always-on JS test of the reactive store drives a 250-line TS re-implementation of the Rust engine (FakeHandle), which demonstrably diverges from the real engine (ignores applyDiff, reverses keyword add/remove order, deletes pending on delete, compares mailboxes order-insensitively, skips watermark gating). The real-wasm smoke test is skip-gated and happy-path only. The headline flicker fix's always-on regression runs against the fake — false-green risk. Retarget the adapter test onto the real wasm handle."
modified: 2026-06-27
reviewed: 2026-06-27
lifecycle: ephemeral
type: ISSUE
status: open
priority: high
depends:
  - path: docs/eph/DESIGN-L2-client-link-reactive-store
  - path: docs/eph/DESIGN-L2-mutation-notification
---

# Test fidelity: the FakeHandle drifts from the real engine

The flicker fix has three test surfaces of very different fidelity:
- Rust engine tests (`convergence.rs`, `entity_store.rs`) — drive the *real*
  engine. High fidelity, strong (`retire_absorbed` ordered-prefix cases,
  `confirm_before_base_update_does_not_revert`).
- TS adapter test (`apps/web/test/entityStoreAdapter.test.ts`) — drives a
  **`FakeHandle`**: a TS re-implementation of the engine (its own
  `retireAbsorbed`/`foldFacets`/`sameFacets`). This is the **only** JS test that
  always runs.
- Real-wasm smoke (`apps/web/test/replicaWasmSmoke.test.ts`) — real engine, but
  `describe.skipIf(!artifactsPresent)`, so it runs only in the `replica-wasm` CI
  job and only over the happy path.

So the fake is load-bearing, and it concretely diverges:

| # | Divergence | Fake | Real | Consequence |
|---|---|---|---|---|
| H1 | `applyDiff` (undo/redo vehicle) | ignored (`foldFacets` returns null) | folded (`apply_message_assertion::ApplyDiff`) | a confirmed undo leaves `hasPending()` true forever in the fake; undo/redo optimism untested |
| H2 | keyword fold order | remove-then-add | add-then-remove (`message.rs`) | opposite result when a keyword is in both add+remove |
| H3 | pending on authoritative delete | deleted | kept (`entity_store.rs:364`) | `hasPending()` lifecycle diverges after deletion (and the fake masks [[L2-outbox-op-lifecycle]] A) |
| H4 | the flicker regression test | runs against the fake's gating | — | proves the *fake* is gated, not the wasm |
| M2 | `sameFacets` mailbox compare | order-insensitive (sorts) | order-sensitive (`Vec`, `ReplaceMailboxes` preserves order) | same-set-different-order absorbed by fake, kept by real |
| M3 | watermark/in-range + sorted insert | omitted (always place, `push`) | `in_range` gated, `insert_sorted` | below-watermark placement + sort order unmodeled |
| M4 | `removeMessageFromViews` deferred | removes from all views | skips deferred | deletion-in-deferred-view diverges |

**Fix (single highest-leverage):** point the adapter test at the **real** wasm
`EntityStoreHandle` (the smoke test already proves it loads synchronously via
`initSync`) — one engine under test, retiring H1–H4 + M2–M4 at once. If a no-wasm
unit path must stay, treat the fake as an explicit stub and make the real-wasm
adapter run a **required (non-skipped)** CI job.

## Coverage gaps (also flagged)

- Deferred (smart-mailbox) views barely tested: no *optimistic* mutation against
  a deferred view, no deletion-in-deferred, no adapter test that registers a
  deferred view at all (Task 4 M5).
- Microtask-count coupling: revert/flicker tests flush with repeated
  `await Promise.resolve()` (`:330`, `:401`) coupled to `settleAll`'s exact
  await-depth — adding an internal await silently makes assertions run early
  (Task 4 M6). Await a deterministic signal instead.
- Missing: multi-op-per-message; reconnect/`viewReplace` re-serve while a pending
  op exists; no-op-confirmation at the adapter layer; `domainCacheMessageUpdated`
  negative assertions are weak (`isInvalidated === false` also satisfied by an
  unrelated early-return) (Task 4 L1/L2).

## Provenance

Four-reviewer Task 4 (H1–H4, M2–M6, L1–L3).
