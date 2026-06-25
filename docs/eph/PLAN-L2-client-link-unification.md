---
scope: L2
summary: "Forward plan: finish unifying the client↔runtime link onto the assertion-replication shape — runtime incremental recompute (U1), the replica as the default client path with the surfaced-failure gap closed (U3), coverage-replaces-sessions (U4), and one BackendApi contracting both seams (U5)"
modified: 2026-06-24
reviewed: 2026-06-24
lifecycle: ephemeral
type: PLAN
depends:
  - path: docs/replication/client-link/L1
  - path: docs/replication/client-link/L3
    section: "5. The failure path and remaining gaps"
dependents: []
---

# Finish the client↔runtime link unification

The client↔runtime link is being unified onto the runtime↔backend
assertion-replication shape ([backend-link L1](../replication/backend-link/L1.md)),
**driven by performance**: the snapshot-push model re-sends the whole view on
every mutation and re-queries per event. The realized half — the opt-in
incremental mail-list delta on the wire and the replica consuming it
([client-link L2 §4](../replication/client-link/L2.md)) — is folded into the
durable docs. This plan tracks what remains. When a slice lands, fold it into the
relevant `client-link` section and remove its `[::state]` marker.

## 1. Remaining slices

- **U1 — runtime recomputes incrementally.** `ViewRegistry` should hold each
  mail-list view's served base and apply the changed message's assertion (read
  one summary, fold, re-project) instead of re-querying the whole page per event.
  Today `build_snapshot` (`crates/posthaste-authority-runtime/src/views.rs`) still
  calls `query_mail_page` on every event and the delta is computed by diffing two
  full snapshots (`sessions.rs::mail_list_delta`) — so cost (2), the per-event
  full re-query, is **not** removed. Lowest risk, no client change, no flag.
- **U3 — the replica is the default client read path.** Promote the WASM replica
  from flag-gated (`VITE_RUNTIME_REPLICA`) to default: finish production wiring +
  real-browser validation, and close the surfaced-failure gap (§3). The
  non-replica snapshot path stays a fallback during transition.
- **U4 — coverage replaces sessions/views.** `open_view` → `subscribe(coverage)`
  + read-through for the initial base; the runtime stops server-side view
  recompute for covered views and serves assertions for the coverage. Sessions
  collapse to connection/coverage state.
- **U5 — one contract.** Fold the client wire onto `BackendApi` (under a neutral
  link name) and retire the bespoke `RuntimeFrame` session/view variants
  superseded by assertions + coverage, so one contract serves both seams.

## 2. Migration safety

Each slice is independently shippable. U1 alone is a behavior-preserving perf
win behind no flag. The whole-snapshot frame is never removed outright — it
remains the initial-load / window-extend / coverage-change frame and the
non-replica fallback until U5 retires only the superseded variants. The delta
frame is additive and capability-gated (`view_delta`), so old and new clients
coexist.

## 3. The load-bearing decision

U2's perf win requires the client to apply assertions (be a near node); U3 makes
the replica the default for all users. The replica is **correctness-safe** to
dogfood (rejected mutations revert via a corrected base,
[client-link L3 §5](../replication/client-link/L3.md)), with one open caveat: the
**surfaced-failure gap** — the user briefly sees success and there is no keyed
`Failed` toast because the mutation already settled `Confirmed` on enqueue.

Closing it is a runtime lifecycle choice:

- **double-settlement** — emit a second keyed `Failed` for the same `mutation_id`
  on provider rejection; or
- **deferred-confirm** — settle non-terminal on enqueue, terminal on the provider
  outcome.

Either way it requires re-establishing the `OperationId`↔session `mutation_id`
link severed at the accept/flush boundary so the background flush can key the
failure back. This is the gate U3 must pass before defaulting the replica on;
until then U1 (runtime-side, no client change) delivers value safely.
