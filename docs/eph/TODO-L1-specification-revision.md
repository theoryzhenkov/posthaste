---
scope: L1
summary: "Ephemeral TODOs for deferred specification revision design questions"
modified: 2026-06-15
reviewed: 2026-06-15
lifecycle: ephemeral
type: TODO
depends:
  - path: docs/runtime/L1
  - path: docs/state/mail/L1
  - path: docs/client/L1
  - path: docs/api/L1
  - path: docs/backend/L1
---

# Specification revision TODOs

## 1. Remote/offline local replica

### 1.1 Question

How should Posthaste support hosted, multi-device, or offline deployments where the authority runtime is not embedded next to the renderer?

### 1.2 Current specification direction

The current specs target bundled application mode. The UI renderer talks to a stateful embedded authority runtime that owns SQLite, provider access, active query views, mutations, and event flow.

No separate local replica is specified for bundled application mode.

### 1.3 Future architecture to design

A future remote/offline deployment may put a local replica behind the same UI-facing runtime contract. That local replica would need:

- partial replicated state
- query/window coverage proofs
- mutation outbox with stable client mutation IDs
- deterministic named-mutation replay/rebase
- origin event reconciliation
- preload and eviction policy
- conflict and settlement semantics

The renderer contract should remain unchanged: UI sends typed actions and renders runtime state.

### 1.4 Query coverage design problem

Do not model local replica certainty as a small enum such as `fresh`, `stale`, `partial`, or `optimistic`.

A future local replica needs structural query/window proof and mutation overlay metadata. The design should answer:

- which `QueryScope` and window are covered
- which origin sequence/version the result is based on
- which rows or ranges are proven complete
- which gaps or unknown boundaries remain
- which pending mutations were applied
- which rows are origin, locally created, locally modified, or pending deletion
- what background fill is needed
- how pending mutations rebase over new origin events

### 1.5 Reference models

[Replicache subscriptions](https://doc.replicache.dev/tutorial/subscriptions) are the closest reference for reactive client views. Replicache's [row-version strategy](https://doc.replicache.dev/strategies/row-version) is useful prior art for computing client views from backend state.

Other local-first references to study:

- [ElectricSQL Shapes](https://electric.ax/docs/sync/guides/shapes) and [`electric-sql/typescript-client`](https://github.com/electric-sql/typescript-client)
- [PowerSync client architecture](https://docs.powersync.com/architecture/client-architecture) and [writing data](https://docs.powersync.com/client-sdks/writing-data)
- [RxDB replication](https://rxdb.info/replication.html) and [conflicts/revisions](https://rxdb.info/transactions-conflicts-revisions.html)
- [WatermelonDB sync backend](https://watermelondb.dev/docs/Sync/Backend) and [`Nozbe/WatermelonDB`](https://github.com/Nozbe/WatermelonDB)

Reference implementations to inspect if this direction becomes active:

- [`rocicorp/todo-nextjs`](https://github.com/rocicorp/todo-nextjs)
- [`rocicorp/todo-row-versioning`](https://github.com/rocicorp/todo-row-versioning)
- [`rocicorp/replicache-examples`](https://github.com/rocicorp/replicache-examples)

### 1.6 Revisit trigger

Revisit when Posthaste needs hosted, multi-device, mobile, browser-offline, or remote-authority deployment modes.
