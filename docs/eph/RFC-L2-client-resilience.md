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
| M40 | Link re-prepare on stale-link 4xx (engine.rs error arm + nearEnd shim signalling) | e2e: reap/restart the link mid-session → stream resumes WITHOUT reload; a suspended-clock test for the sleep>5min shape |
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
