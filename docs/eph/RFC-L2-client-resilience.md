# RFC-L2-client-resilience — the client converges from any state

Status: DRAFT for owner ratification (2026-07-03). Evidence: the client-fragility
audit (5 subsystem maps + 2 symptom hunts + 14 adversarial verifications; 2
CONFIRMED, 3 PLAUSIBLE-high, 9 refuted — refutations recorded below).

## 1. Field symptoms

- S1: views sometimes stop updating until reload/navigation.
- S2: trashing a message leaves the sidebar unread counter stale.

## 2. Diagnosis — the fragility map

The web client's update delivery is one chain:

```
SSE/WS link → frame demux → EntityStoreController.enqueue → worker port → wasm store
           → drainAndEmit → { synthesized view frames → React lists,
                              writeMailboxCount → react-query counter cache }
```

Every hop is **edge-triggered** and several hops can fail **permanently and
silently**. The D44 lesson (level-triggered reconciliation) was applied to seq
handling *within* a healthy link, but not to the lifecycle of the link, the
worker, or the subscription binding around it. Verified failure modes:

| # | Verdict | Mechanism | Site |
|---|---------|-----------|------|
| F1 | CONFIRMED | Stream reconnect after a dead link (idle-reaped at SESSION_IDLE_TTL=300s — every laptop sleep >5min — or daemon restart) → 404 on the subscribe GET → engine classifies Permanent → `run()` returns → **all live updates halt until reload**. The engine cannot re-prepare a link. | link-near-end `engine.rs:359-377`; `nearEnd.ts:114`; `runtime_stream/links.rs:103` |
| F2 | CONFIRMED | Every base-frame path store op is `void this.enqueue(...)` with a rejection-swallowing tail and no call-site catch — a failed ingest/drain (dead worker, wasm error) drops the update with **no log, no retry, no invalidation**. | `entityStoreAdapter.ts:785,767,600,506` |
| F3 | PLAUSIBLE-high | Watchdog worker respawn (M31) wipes ALL wasm store state (views, bases, accepted mutations) and replays only the single timed-out call — no re-seed; the controller isn't told. | `workerStorePort.ts` respawn path |
| F4 | PLAUSIBLE-high | Dead-worker latch is permanent (restart budget exhausted, or any worker `error` event) — no mid-session recovery. | `workerStorePort.ts:144-155,200-238` |
| F5 | PLAUSIBLE-high | Startup race: the one shared frame subscription can bind to the pre-wasm base adapter for the whole session (ensureStream fires when the link resolves, not when the wasm adapter installs). | `runtimeStream.ts:46`, `linkClient.ts:101`, `adapter.ts` |

**Design asymmetry that turns stream death into S2:** list rows are optimistic
(the fold removes the row instantly) but mailbox counts are authority-only BY
DESIGN (`entity_store.rs:42` — "a count delta from the authority is the only
path"), delivered over the same stream. A healthy stream makes the round-trip
invisible; a dead one produces exactly "row gone, badge stale". The audit
REFUTED the idea that authority-only counts are themselves the bug — the bug is
that stream health is a silent single point of failure. Also refuted: the
hardcoded `skipStoreOwned=true` (a real observation, but a recovery path
exists); several `writeMailboxCount` no-op theories (the seeding actually
covers them); the rAF-coalescing starvation (bounded by the 256-frame cap).

## 3. Decisions (proposed)

- **D110 — Level-triggered self-healing at every layer** (D44 promoted from
  frames to lifecycle). Every layer must converge from any state rather than
  rely on never missing an edge:
  (a) *Link*: a stream-open failure indicating a stale/absent link (404/410)
  clears `prepared`+token and re-runs `open_link` — re-prepare, not Permanent
  halt. `Permanent` is reserved for auth-refused (401/403 after refresh).
  (b) *Worker*: a respawn (watchdog or error-event) runs a **re-seed
  protocol** — re-register all views, replay the durable pending set, request
  fresh base frames (viewReplace) for every open view; the dead latch is
  replaced by bounded-backoff respawns + a visible degraded state.
  (c) *Binding*: the shared frame subscription re-binds on adapter install
  (readiness handshake), closing the F5 race.
- **D111 — No silent drops.** Every `void enqueue(...)` gains a failure path:
  log + a compensating invalidation (a dropped ingest ⇒ schedule a view
  refresh + counter refetch). Failures route into recovery, never into
  nothing.
- **D112 — One client reconcile pass.** A single connection-health FSM
  (healthy/degraded/recovering); on ANY recovery edge (link re-prepared,
  worker respawned, tab foregrounded after hidden) it runs one reconcile:
  re-request base frames for open views + refetch `queryKeys.mailboxes` +
  invalidate stale-prone caches. The client twin of the server's
  level-triggered reconciler.
- **D113 — Counters keep authority-only as primary** (ratified design), with
  the reconcile pass (D112) as the always-correct fallback: counts are
  refetched on every recovery edge, so a missed delta can never persist past
  a reconcile. (Rejected alternative: making `skipStoreOwned` health-derived —
  more moving parts for the same guarantee.)
- **D114 — Degraded state is visible.** A subtle indicator when the FSM is not
  healthy (reuses the M31 reload-toast pattern), so silent staleness becomes a
  user-visible, self-explaining state.

## 4. Migration steps

| Step | Scope | Gate |
|------|-------|------|
| M40 [::done 2026-07-04] | Link re-prepare on stale-link 4xx. LANDED. Residual (M42/M44): re-prepare resumes the stream + self-maintained lists; server-re-served views (smart mailboxes/search) + linkClient's cached linkId need the reconcile pass — on_reset deliberately NOT fired (false semantics for a fresh link). | e2e: reap/restart the link mid-session → stream resumes WITHOUT reload; a suspended-clock test for the sleep>5min shape |
| M41 | enqueue failure paths (D111): catch + log + compensating invalidation at the 4 sites | test: a store op that throws triggers the invalidation, not silence |
| M42 | Worker re-seed protocol + dead-latch removal (D110b) | test: kill the worker mid-session → views re-populate + pending set replays + counts recover |
| M43 | Subscription readiness handshake (D110c) | test: the F5 startup interleaving now re-binds |
| M44 | The reconcile pass + health FSM (D112) wired to link/worker/visibility recovery edges; counter refetch on reconcile (D113) | e2e: sever the stream, mutate on "another device", recover → views AND counters converge |
| M45 | Degraded-state indicator (D114) | UI test |

M40 is the single highest-value fix (it likely kills most day-to-day S1/S2
occurrences — every laptop sleep currently halts the client permanently) and is
small; it should land first and could ship alone as a hotfix nightly.

## 5. Rejected / deferred

- R-counters-optimistic: client-side optimistic count deltas — rejected;
  authority-only is simpler and correct once delivery is self-healing (D113).
- R-two-way-binding-audit: rewriting the domain-cache handler routing — not
  justified by evidence; the handler layer survived adversarial review.
- Deferred: the 8 unverified low-ranked findings (rAF starvation in hidden
  windows, single-sink multi-subscriber shape, first-caller-wins link scoping,
  D49-reset invalidation gap, …) — recorded in the audit transcript; revisit
  only if symptoms persist post-M44.

## Part 2 — structural revamp (owner-ratified 2026-07-04)

### Decisions
- **D115 — One TS reactive store (the dumb mirror).** The wasm replica stays the
  sovereign source of domain truth and ALL computation (folds, counts, view
  projection). Because the replica lives in a worker (React needs synchronous
  getSnapshot) and infra state (connection health) is not domain state, a
  main-thread mirror is unavoidable — today it exists informally
  (lastProjectionJson maps, setQueryData count writes, ad-hoc subscriber sets:
  the audit's drift seam). D115 collapses these into ONE store module with
  useSyncExternalStore hooks: view projections (per viewKey), mailbox counts
  (per account), connection health (the D112 FSM's home). The store holds no
  logic — latest projected state + notify.
- **D116 — react-query shrinks to request/response.** Mailbox STRUCTURE
  (names/roles/hierarchy) stays a query; live COUNTS move to the store slice;
  the sidebar composes both. No event-driven setQueryData for live state.
- **D117 — Adapter decomposition along the node algebra.** entityStoreAdapter's
  God-object splits into TS modules mirroring kernel/projector/link anatomy,
  with the D112 health statechart (hand-rolled first; XState only if it grows)
  at the center.
- **D118 — Codegen the event boundary.** From asyncapi: topic→payload map + an
  exhaustive handler registry (unhandled topic = compile error), extending the
  schema.gen pipeline.
- **D119 — Deterministic client testkit + boundary enforcement.** Fake
  transport/worker/virtual-time harness (the client twin of posthaste-testkit);
  promote the hand-rolled boundary check scripts to enforced layer rules
  (eslint-plugin-boundaries or dependency-cruiser); a thin 5-scenario
  Playwright smoke.
- **R-rows:** external local-first frameworks (replace the verified moat),
  Effect-TS (paradigm tax), state-library/framework migration (no evidence).

### Migration
| Step | Scope | Gate |
|------|-------|------|
| M46 [::done 2026-07-04] | The reactive store LANDED (counts fully migrated; view projections dual-write — M49 residual: useRuntimeMailListView still consumes synthesized frames). | all web tests pass; no setQueryData-for-counts remains (grep); counts update end-to-end via the store |
| M47 | Event-boundary codegen (D118) | unhandled-topic = compile error; coverage matrix generated |
| M48 | Client testkit + boundaries lint + Playwright smoke (D119) | the 5 scenarios run deterministically |
| M49 | Adapter decomposition + health statechart (D117, absorbs M44's FSM home) | boundaries lint enforces the seams |
| M50 | react-query shrink completion (D116) — remaining event-driven cache patches migrate or justify | audit grep: domain-cache handlers only invalidate request/response queries |

Sequencing: M40 (Part 1) ships first as the hotfix; M46 proceeds in parallel
(owner-directed); M47/M48 next (they harden all later gates); M49/M50 ride
with the remaining resilience steps (M42/M44).
