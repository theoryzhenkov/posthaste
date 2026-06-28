---
scope: L2
summary: "Render-layer flicker probe (RenderProbe/RenderLog) driving the REAL entityStoreAdapter + WASM + hook + render at its injection seams; retires the duplicated Rust ReplicaProbe adapter-port"
modified: 2026-06-27
reviewed: 2026-06-27
lifecycle: ephemeral
type: DESIGN
depends:
  - path: docs/testing/L1
  - path: docs/eph/PLAN-L2-testkit-roadmap
  - path: docs/eph/DESIGN-L2-client-link-reactive-store
  - path: docs/eph/DESIGN-L2-message-authority-version
  - path: docs/runtime/L1
dependents:
  - path: docs/testing/L1
  - path: docs/eph/PLAN-L2-testkit-roadmap
---

# Render-layer flicker probe (Layer D)

## Principle: instrument, don't duplicate

The testkit drives **real production code**; fixtures supply **external inputs
at seams** and record **observables**. No hand-ported copy of production glue.

The existing testkit is mostly already this shape:

| Component | Role | Verdict |
|---|---|---|
| `posthaste_link_replica::EntityStore` (Rust = WASM) | reconciliation/optimism logic | **shared** — same code the browser runs |
| `RuntimeHarness` → `AuthorityRuntimeBuild` | the runtime | **real** — `settle`/`watch_view`/`open_capture` are recorders, not ports |
| `Harness` (store + `MailService`) / `StalwartFixture` | store + real Stalwart | **real** |
| `GmailImapFixture` / `FakeRuntimeAdapter` | mock Gmail IMAP / mock runtime transport | **legitimate external-boundary mocks** — real `imap-client`/adapter under test |
| `ReplicaProbe` (`replica_probe.rs`) | frame→store glue | **DUPLICATED — retire** (hand-ports `entityStoreAdapter.ts`; already diverges; gave false confidence) |
| `FlickerLog` / `reverts` / `RenderSnapshot` | flicker detector | **keep** — test-only assertion logic, applied to real outputs |

The one duplicated smell is `ReplicaProbe`: a Rust copy of the TS
`entityStoreAdapter.ts` controller. It mirrors `openMailListView`,
`onBaseFrame` (viewSnapshot/viewReplace → ingest+`setViewRows`; `message.updated`
→ ingest; `mutationNotification` → `settle`), and the helper ports
(`projectionBatchFromRows`, `toStoreRow`, `ingestMessageEvent`). It must be
hand-synced and has **already diverged**: watermark hardcoded `None` vs
`watermarkFromSnapshot`; no durable-outbox rehydration; no `writeMailboxCount`;
no synthesized `viewReplace` back out. Driving the **real** `EntityStore` with a
**divergent orchestration** only proves "the store behaves given *my* sequence,"
not "given the production sequence" — and indeed the flicker bugs were caught by
the real-adapter Layer C tests while `ReplicaProbe` "structurally missed them."
That false confidence is the textbook failure of duplicated test code.

Layer D replaces `ReplicaProbe`'s role by driving the **real** adapter. It does
not add a second port.

## Problem

Two residual UI flickers after the replica fixes shipped in `v0.2.0-nightly.21`:

1. **Undo/redo flicker** — brief flicker on undo/redo. Suspect: the `undoHead`
   machinery.
2. **Whole-view flash of past state** — on mailbox-move/delete the whole view
   briefly renders as it was at last load; the flashed set is *exactly* the items
   deleted since the last view reload (reload resets it); appears with no
   mutation echo required ("just rendering"). Suspect: React rendering.

Both live **above the replica**, which the existing fixture proves clean for undo:
a real-WASM probe (`applyDiff` undo of a move) holds the inbox row through an
equal-version stale re-serve — fix (a)'s version-gated retire is generic over the
assertion kind. The undo flicker is **not** the replica-optimism class.

The mail list renders purely from `query.data` at `messageQueryKey`; no
animation; **only** `useRuntimeMailListView` writes that cache; the effect's
`operationEntry`/`queryKey`/`preparedSearchQuery` deps are stable across a delete
(no re-open). `placeholderData: (previousData) => previousData` is present
(`MessageList.tsx:130`) but only fires on a query-key change. The undo runtime
path (`build.rs::run_apply_diff`: `pop_undo_by_seq` → `dispatch_named_mutation`
→ `push_redo` → `emit_history_frame`) runs the runtime's own outbox overlay
(accept/forward/retire) — an extra frame + history nav the `undoHead` intuition
points at.

## Layering

| Layer | Drives | Sees | Status |
|---|---|---|---|
| A — `FrameCapture` | runtime in-process | raw `RuntimeFrame` stream | realized (recorder) |
| B — `ReplicaProbe` | captured frames → real `EntityStore` via a **ported adapter** | per-row projection trajectory | **retire** — duplicated divergent glue |
| C — real-WASM store tests | frames → real WASM `EntityStoreHandle` (handle API directly) | per-row projection trajectory | keep (store-level) |
| **D — `RenderProbe`/`RenderLog` (this doc)** | **real `createEntityStoreAdapter` + real WASM + real `useRuntimeMailListView` + React render** | `query.data` per frame + rendered row-set per commit | planned |

C and D both drive real code; C is store-level (faster, no React), D is the
adapter+render level. B is the only duplicated layer and is superseded by D.

## Seams + fixture model

The adapter is already designed for injection (`EntityStoreAdapterDeps`):
`base`, `makeHandle`, `outbox`, `queryClient`, `now`. Layer D wires those to
test doubles and drives the **unmodified** `createEntityStoreAdapter`:

- **`base` = `FakeRuntimeAdapter`** — the external transport seam. `emitRuntimeFrame`
  is where a fixture delivers a frame; the adapter cannot tell a live frame from
  a replayed one.
- **`makeHandle`** — real WASM `EntityStoreHandle` (the shipped store).
- **`outbox`** — a real or fake `OutboxStore` (durable-intent rehydration is part
  of what's under test, so prefer the real one).
- **`queryClient`** — a test `QueryClient` (the observable the renderer binds).
- **`now`** — controllable clock.

**Fixture = external inputs at these seams**, in two modes:

1. **Record/replay** — drive the real `RuntimeHarness` (Rust) through a scenario,
   capture the `RuntimeFrame` stream with `FrameCapture` (Layer A), serialize it,
   then replay it frame-by-frame through `FakeRuntimeAdapter.emitRuntimeFrame`.
   This drives the real adapter with the **real runtime's** frame sequence
   (including the undo's `mutationHistory` + overlay-retire ordering) with no
   live Rust→JS bridge.
2. **Authored** — hand-craft a frame sequence at the same seam for a targeted
   scenario (e.g. an equal-version stale re-serve).

**Observables** (recorded, not asserted-against-internals): `queryClient.getQueryData`
after each driven frame (the **CacheProbe** aspect — scheduling-independent,
proves the stale-write cause) and each rendered row-set per commit (the
**RenderProbe** aspect — catches `placeholderData`/render-timing a correct cache
would hide). Plus the adapter's emitted `viewReplace` frames on the sink.

`FlickerLog.reverts` / `assertNoFlicker` (kept from `ReplicaProbe`, applied to
**real** outputs) with three rules:

| Rule | Detects | Maps to |
|---|---|---|
| **row-revert** | a row field reverts to a prior value after a mutation set it, then returns | keyword/move class (regression guard) |
| **disappear-reappear** | a row removed by a mutation reappears for ≥1 commit, then is removed again | delete-flash |
| **snapshot-regression** | the whole row-set equals a *prior* commit's set after the render advanced past it (unprompted by a navigation/undo) | whole-view flash |

## Division of work

1. **`RenderProbe` + `RenderLog` helper** (`apps/web/test/renderProbe.tsx`):
   wires `createEntityStoreAdapter` over a `FakeRuntimeAdapter` base + real WASM
   handle + real outbox + test `QueryClient`; renders a host component using
   `useRuntimeMailListView`; records cache + render. **Red-first:** feed a
   deliberate stale `setQueryData` between two correct states and assert the
   detector fails (proves it sees a flash).
2. **Issue 2 repro** (`apps/web/test/mailListRenderFlicker.test.tsx`): open a
   view, delete a row, record the cache + render sequence. Stale `query.data` ⇒
   cache-write bug; render-only flash ⇒ `placeholderData`/timing.
3. **Issue 1 repro** (`apps/web/test/undoRenderFlicker.test.tsx`): record the
   real undo frame sequence from `RuntimeHarness` (mutate → capture
   `mutationHistory` → `applyDiff { undoOf }` → capture), replay through the
   real adapter, assert no flicker.
4. **Retire `ReplicaProbe` adapter-port**: move `FlickerLog`/`reverts`/
   `RenderSnapshot`/`RenderedRow` to live above the real adapter (or a shared
   detector module both Rust-property-tests and the TS probe can use), and
   delete the ported `open_view`/`apply_frame`/`accept_mutation` glue + helper
   ports from `replica_probe.rs`. Store-level property tests (P5) that call
   `EntityStore` directly are unaffected (no glue involved).

## Step 1 landed (2026-06-27)

`apps/web/test/renderProbe.tsx` + `renderProbe.test.tsx` (9 tests, red-first):
the detector catches disappear-reappear / keyword / move / read reverts +
whole-view snapshot regression, passes clean monotonic delete + flag set; the
probe drives the real adapter + real WASM, opens + records served rows, and a
flag toggle holds through confirm + base catch-up (Bug 1a, green).

Implementation gotchas for steps 2/3:
- **`screen` doesn't work** with per-file happy-dom (`dom-env`): `screen`
binds at import time, before `beforeAll` registers the DOM, so its query helpers
are throwing stubs. Use the `getAllByTestId` bound to the rendered `container`
returned by `render()`, or read the cache directly.
- **React Query's `notifyManager`** defers the observer re-render past a
synchronous `act`. Each `emitFrame`/`runMutation`/`writeCache` must `await new
Promise((r) => setTimeout(r, 0))` *inside* the `act` so the observer commits and
the spy captures the post-update render, not the pre-update one.
- **The hook's open writes the cache from a detached** `openMessageListView()
.then(setQueryData)` microtask (outside `act`). Wait on the cache (`getQueryData`,
act-free), then flush one `act(setTimeout(0))` to commit the observer render.
- **`useInfiniteQuery` requires `getNextPageParam`** or the observer throws on
the first cache update (`hasNextPage` → `options.getNextPageParam` undefined) and
`query.data` never advances.
- **Refs cannot be written during render** (`react-hooks/refs`): the spy is
  captured in a `useEffect` after commit (which also correctly reflects only
  *committed* renders, incl. `placeholderData` states).

## Step 2 landed (2026-06-27)

`apps/web/test/mailListRenderFlicker.test.tsx` (3 tests) drives real
`message.replaceMailboxes` moves through the probe and records the render
trajectory. Findings:

1. **A plain optimistic move is clean (no flash).** Moving m1→archive then
   m2→archive monotonically removes the rows ([m1,m2,m3] → [m2,m3] → [m3]);
   the adapter emits exactly one correct `viewReplace` per move and re-applies no
   stale snapshot. So issue 2's "flash of past state with no mutation arrived"
   is **not** in the mail-list optimistic-move path — the move emit does not
   re-serve the load-time snapshot.
2. **The move-flicker class (Bug 1b unguarded) reproduces, red.** After a move +
   confirm + base-catch-up (op retires), a stale `message.updated` re-serving the
   moved message in the origin mailbox **with no `version`** clobbers the
   retired op's base → the row reappears → presence flicker. This is the equal-
   version/unguarded tail the `.20/.21` work addressed, now reproduced through the
   real adapter + real WASM + real hook + real render (above the replica).
3. **The version guard (fix a) holds end-to-end, green.** The same stale re-serve
   at `version=1` (older than the held `version=2`) is rejected → the row stays
   archived. Fix (a) verified at the render layer, not just the replica.

So issue 2's no-mutation flash is **not** the optimistic move. The remaining
suspect is `placeholderData: (previousData) => previousData` during a mailbox
**switch** (a query-key change) — the one mechanism that flashes "previous data"
with no mutation. The probe hardcodes one view, so this is step 2b: extend the
probe to switch `selectedView` mid-flight (or a focused host) and record whether
the disabled `useInfiniteQuery` flashes the prior mailbox's data during the key
transition.

### Option iii landed (synced 2026-06-27 from views-stability)

`perf(runtime): retire the per-event mail-list re-serve` landed on main: the
runtime now fires only `message.updated` for mail-list mutations and **skips**
`send_recomputed_replace` for `ViewKind::MailList` (the client self-maintains
from the firehose); resync paths recompute open views first (`refresh_open_views`)
so a stale mail-list snapshot can't replay. This **retires the runtime as a
stale mail-list re-serve source** — so for issue 2, a runtime `viewReplace` flash
on a mutation is now structurally impossible. That narrows the remaining suspect
to the client/adapter (step 2 found the optimistic move clean there) or the
`placeholderData` view-switch path (step 2b). The testkit Rust suite is green on
the new contract (`view_settlement` asserts `assert_message_updated_notification`
+ `assert_view_not_recomputed`; the live trio + `mutation_flicker` pass
unchanged), and the web probe suite (20 tests) is green post-rebase. NB: the
`view_settlement` test name still says "...recomputes_the_touched_view" but its
body now asserts the view is **not** recomputed (option iii) — a historical-name
nit worth a rename.

## Open questions

1. **Replay pacing.** Record/replay flattens real-time pacing; a flicker that
   depends on React concurrent-scheduling timing (not frame content/order) may
   not reproduce under deterministic single-frame `act` driving. Fallback: the
   `CacheProbe` record of `query.data` per frame is scheduling-independent and
   still proves the stale-write cause; a render-only flash that needs real
   pacing escalates to a built-web Playwright rung (verification ladder rung 3).
2. **`snapshot-regression` vs. legitimate undo.** A row-set legitimately equals
   a prior commit *after an undo* (undo is a regression by intent). The rule
   keys off **mutation intent**: a snapshot-regression is a flash only when
   *unprompted by a navigation/undo* — interleaved with a mutation that should
   have removed/changed the rows. The probe records the driven action alongside
   each render to disambiguate.
3. **Outbox realism.** Whether to use the real `OutboxStore` or a fake for
   durable-intent rehydration. Prefer real (it is part of the render path under
   test); fall back to fake only if the durable store's async surface blocks
   deterministic single-frame driving.
4. **Where this folds back.** On landing: add a `render-flicker-contracts`
   assertion to `docs/testing/L1` §3 ("the mail-list render sequence through a
   mutation/undo contains no transient stale row-set, driven through the real
   adapter"); add a Layer D row to the verification ladder; mark `ReplicaProbe`
   retired in the roadmap; delete this design doc.
