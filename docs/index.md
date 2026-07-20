---
title: Technical reference
description: The durable, layered technical specs — architecture, API, backend, client, mail state, testing, and UI.
modified: 2026-07-19
reviewed: 2026-07-19
---

The durable, layered technical specs live in domain directories. Each domain
is authored at the scope levels it needs (L0 orientation → L1 contract → L2
structure → L3 implementation reference):

- [Architecture](architecture/L1-architecture.md): the integrated app — one
  Rust backend as the only evaluator, a TypeScript frontend rendering live
  query results, and the localhost protocol between them.
- [API](api/L1-api.md): the one integration surface — queries, commands, the
  event stream, and binary resources; the same contract for the frontend and
  every external consumer (user scripts, and the CLI/MCP tools as they are
  rebuilt on it).
- [Backend](backend/L1-backend.md): internal structure — the domain service
  over the SQLite store, provider gateways, account runtimes, the outbox,
  events and the store generation, query evaluation, command execution.
- [Client](client/L1-client.md): the frontend contract — the facade, live
  queries, commands as verbs, ephemeral state, and hackability.
- [Mail state](state/mail/L1.md): canonical mail state, derived projections,
  query evaluation, and conversation freshness.
- [Crate topology](architecture/L2-crate-topology.md): the workspace crate
  set, ownership, and dependency direction — the core crates under
  `crates/` and the app crates under `apps/client`.
- [Testing](testing/L0.md): the behavior-contract coverage model, the shared
  `posthaste-testkit` harness, and the verification ladder.
- [UI](ui/L0.md): the mail shell's navigation model — view kinds, pane
  focus, and the keyboard contract.

## Decision records and design history

The durable specs above record *what is true now* (drafts marked `state:
draft` record what is being built). The reasoning, deviations, and forward
plans live as dated records in [`eph/`](eph/INDEX.md) — RFCs, audits, design
notes, and reality ledgers. The pivot from the split client/runtime model to
the integrated app is recorded in
[RFC-L2-mirror-client.md](eph/RFC-L2-mirror-client.md); the pre-pivot spec
set survives on the `legacy/distributed-model` branch, and the split-model
code's final pre-deletion tree on `legacy/split-model-final`. Open technical
debt is tracked in [`issues/`](issues/L2-index.md).
