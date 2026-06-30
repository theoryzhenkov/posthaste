---
scope: L2
summary: "Move the client replica (WASM entity store + per-event ingest/projection + durable outbox/undo) off the UI main thread into a Web Worker, behind a StorePort abstraction, so a large sync (repair re-sync, first account add, big mailbox) never freezes rendering or starves the navigation read path. Stage 1 of the repair-blocking fix; Stage 0 (frame coalescing + a Syncing state) already shipped."
modified: 2026-06-30
reviewed: 2026-06-30
lifecycle: ephemeral
type: DESIGN
status: draft
depends:
  - path: docs/eph/DESIGN-L2-client-link-reactive-store
  - path: docs/replication/client-link/L2
dependents:
  - path: docs/issues/L2-adapter-reproject-all
---

# Replica worker isolation: get the data layer off the UI thread

## Context

After "Repair & restart", `mail.sqlite` is rebuilt empty and the supervisor does a
full re-sync of every account, streaming a large burst of `message.updated`
frames. The client processes them **on the UI main thread**: the WASM entity
store (`entityStoreAdapter.ts` → `EntityStoreController` → `EntityStoreHandle`)
ingests + re-projects each change, and `useDaemonEvents` invalidates React Query
caches — per event. During a full replay this saturates the main thread, so React
can't paint, the accounts/mailbox bootstrap can't resolve, and counts sit at `0`.

This is structural, not incidental — the same freeze appears on a first account
add, a big mailbox, or a bursty provider. Four seams compound (see the troubleshooting
notes): (1) **the reactive data layer lives on the UI thread**; (2) push-only
transport with no backpressure; (3) two overlapping per-event consumers
mid-migration; (4) bulk hydration reuses the live-delta channel. Seam (1) is the
root.

**Stage 0 (shipped):** coalesce the `message.updated` burst into one
ingest+projection per animation frame (`entityStoreAdapter.ts`, `SYNC_FLUSH_CAP`),
and show a "Syncing…" empty-state instead of bare `0`. That reduces redundant
re-projection but a single large batch still runs on the UI thread — relief, not
a fix.

**Stage 1 (this doc):** move the replica off the UI thread so data work *cannot*
block rendering, regardless of sync volume.

## Options

### Option A: Replica in a Web Worker (recommended)
- **Approach:** the WASM entity store + per-event ingest/drain/projection + the
  durable outbox/undo (IndexedDB, available in workers) run in a dedicated
  Worker. The main thread posts inbound frames and mutation requests to the
  worker; the worker posts back projected `viewReplace`/count frames and mutation
  receipts. React, React Query, and the renderer stay on main.
- **Pros:** the UI thread stays responsive under any sync volume — directly
  delivers "fetching messages shouldn't block the app". Decouples producer rate
  from UI (natural backpressure: main posts, worker drains at its pace). The
  WASM boundary is *already* JSON-serialized (`ingestBatchJson`/`projectViewJson`),
  so crossing a worker boundary is the same payload, just a `postMessage`.
- **Cons:** the store API becomes **async**. Today `openMailListView` seeds +
  projects synchronously and `runMutation` captures the invertible undo diff
  synchronously before folding; both become round-trips. The bun test harness
  uses synchronous WASM init and asserts projections inline — needs an async-safe
  shim. Cross-thread debugging is harder.
- **Effort:** medium-high (the boundary refactor is the bulk; the WASM move
  itself is small because it's already JSON-in/JSON-out).

### Option B: Stay on the main thread, chunk + yield
- **Approach:** keep the store on main but make bulk processing cooperative —
  process the buffered batch in time-sliced chunks that yield to the event loop
  (e.g. `scheduler.postTask`/`isInputPending`), so paint + input interleave.
- **Pros:** no async boundary, no worker, smaller change; reuses Stage 0's buffer.
- **Cons:** doesn't *remove* the contention — it interleaves it. The UI still
  janks (work is on the same thread), just in smaller hitches. A pathological
  burst still degrades responsiveness; it papers over seam (1) rather than fixing
  it. Yield points are fiddly and easy to regress.
- **Effort:** low-medium.

### Option C: Worker for the store only; orchestration stays on main
- **Approach:** move just the `EntityStoreHandle` (WASM) into a worker but keep
  the `EntityStoreController` orchestration (frame routing, outbox, undo) on main,
  RPC-ing each store call to the worker.
- **Pros:** smaller conceptual move.
- **Cons:** worst of both — every `ingest`/`drain`/`project` becomes a
  chatty async round-trip while the orchestration (and its per-event work) stays
  on the UI thread. High message volume, little benefit. Rejected.

## Recommendation

**Option A**, reached via a **`StorePort` abstraction** so the migration is
incremental and reversible:

1. Define `StorePort` — the async interface the controller needs from the store
   (register/seed view, ingest batch, drain+project dirty views, settle, drain
   retired, outbox/undo ops). All methods `Promise`-returning.
2. Implement `InProcessStorePort` — the current in-thread WASM handle behind the
   async interface (await of already-synchronous work). Ship this first: pure
   refactor, behavior parity, keeps the bun harness synchronous-ish (resolved
   promises). This de-risks the boundary independent of the worker.
3. Implement `WorkerStorePort` — the same interface over `postMessage` to a
   worker that owns the WASM module + outbox/undo. Land behind a flag, validate,
   flip the default.

Stage 0's coalescing stays and becomes the batching unit posted to the worker
(one message per animation frame, not per event), so the cross-thread chatter is
also bounded.

## Implementation outline

**New boundary (`StorePort`):**
```
interface StorePort {
  registerView(viewId, spec): Promise<void>
  seedView(viewId, rows, watermark): Promise<void>
  ingestBatch(updates): Promise<void>
  drainDirtyAndProject(): Promise<{ views: ProjectedView[]; counts: CountDelta[]; retired: string[] }>
  settle(clientMutationId, verdict): Promise<void>
  // outbox/undo move with the store:
  enqueueOutbox(record): Promise<void>
  removeOutbox(ids): Promise<void>
  // …
}
```
The controller programs only against `StorePort`. `drainDirtyAndProject` returns
*all* the per-flush outputs in one round-trip (views to emit + counts to write +
retired ids), so a coalesced flush is a single message each way.

**Data flow (worker mode):**
```
runtime frames ──▶ main: coalesce (rAF) ──postMessage(batch)──▶ worker
                                                                  │ ingest + drain + project
main: emit viewReplace + write counts ◀──postMessage(result)──── ┘
        │
        ▼ React (Query cache + list re-render)
```
Mutations: `runMutation` → main posts the translated mutation → worker folds
optimism, captures the invertible diff, persists outbox → posts back the
optimistic projection + receipt → main emits it. Already a `Promise`; the only
change is awaiting the worker instead of a sync call. First optimistic paint is
one round-trip (sub-frame on a warm worker).

**What moves vs stays:**
- Worker: WASM `EntityStoreHandle`, ingest/drain/project, durable outbox + undo
  history (IndexedDB), settlement/retire.
- Main: frame subscription + Stage-0 coalescing, mutation translation
  (`parseMessageMutation`), React Query, renderer, the `viewReplace`/count sink.

**Test harness:** `wasmUtil.ts` keeps the synchronous init for the
`InProcessStorePort`; bun tests construct the controller with the in-process port
and `await` (microtask) instead of relying on sync returns — most already
`await Promise.resolve()`. A small set of worker-transport tests can use a
same-thread `MessageChannel` fake. No real worker in unit tests.

**Phasing:**
- P1: extract `StorePort` + `InProcessStorePort`; controller refactor; parity
  (the existing 14 adapter tests + the WASM smoke tests are the gate).
- P2: `WorkerStorePort` + the worker entry; flag-gated; manual repair-sync soak.
- P3: flip default to worker; keep in-process as the SSR/no-worker fallback.

**Risks / open questions:**
- Async mutation optimism latency (first paint) — measure on a warm worker;
  pre-warm the worker at app start.
- IndexedDB ownership moving into the worker — confirm no main-thread reader of
  the outbox/undo stores remains (grep `replicaDatabase`/`outboxStore`/
  `undoHistoryStore` consumers).
- Large `viewReplace` snapshots crossing the boundary — structured clone of JSON
  strings; if it shows up, switch to delta frames (the reactive-store design
  already contemplates `viewDelta`).
- Worker + Tauri webview: confirm module-worker support in the WKWebView /
  WebView2 targets.

## Relationship to other work

- Closes the root of `docs/issues/L2-adapter-reproject-all` at the architectural
  level (the reverse-index already removed the all-views scan; this removes the
  *thread* contention).
- Makes seams (2)–(4) optimizations rather than freezes: backpressure and a
  distinct bulk-hydration path become "reduce total work", not "unblock the UI".
- Built on the `client-link-reactive-store` model; the `viewDelta` frame type it
  defines is the escape hatch if full-snapshot transfer is too heavy.
