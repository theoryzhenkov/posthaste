---
title: Posthaste
description: Your mail, delivered at Posthaste
modified: 2026-07-04
reviewed: 2026-07-04
---

# Posthaste specs

The durable, layered technical specs live in domain directories. Each domain is
authored at the scope levels it needs (L0 orientation → L1 contract → L2
structure → L3 implementation reference):

- [Mail state](state/mail/L1.md): canonical mail state, derived projections, query evaluation, and conversation freshness.
- [Runtime](runtime/L1.md): UI-facing runtime contract for the bundled application, embedded authority server, and future deployment adapters. Sub-domains: the [runtime adapter](runtime/adapter/L1.md) (the client-facing Api/Link surfaces), [runtime internals](runtime/internals/L1.md) (assembly, links, lifecycle), and [mutations](runtime/mutations/L1.md).
- [Replication](replication/L1.md): coherent links — the optimistic up-channel, authoritative down-channel, and confirmation-watermark convergence that move state between client, runtime, and authority server. Two seams have their own sub-domains: the [client↔runtime link](replication/client-link/L1.md) (the device replica) and the [runtime↔authority-server link](replication/authority-server-link/L1.md) (the AuthorityServerLink seam).
- [Client](client/L1.md): the renderer's boundary over runtime state — the runtime adapter facade, view/mutation hooks, the main-thread reactive live-store, and the wasm replica hosted in a Web Worker.
- [API](api/L1.md): external `/v1` HTTP and SSE contract over those projections. The route inventory is generated: [endpoints](api/endpoints.md).
- [Authority server](authority-server/L1.md): the far node's service, store, provider, account-runtime, event, and API implementation boundaries.
- [Crate topology](architecture/L2-crate-topology.md): the workspace crate set, ownership, dependency hierarchy, role binaries, and the wasm-pure frontier.
- [UI](ui/L0.md): the mail shell's navigation model — view kinds, pane focus, and the keyboard contract. Partial: only the navigation model (L0) and keyboard shortcuts (L1) are authored; the remaining UI sections are a follow-up.
- [Testing](testing/L0.md): behavior-contract coverage model, the shared `posthaste-testkit` harness (`StalwartFixture`, the `mock-gmail` label-model fixture, runtime-in-harness), the client testkit (`apps/web/test/harness`), and the verification ladder. The remaining forward contract (`posthastectl`) is in the [testkit roadmap](eph/PLAN-L2-testkit-roadmap.md).

Task-oriented, tool-facing guides (as opposed to the layered specs above) live
alongside the specs:

- [Scripting quickstart](https://posthaste.theor.net/docs/scripting-quickstart): automate Posthaste from a shell script with no protocol code — the `/v1/events` tap plus the one-vocabulary apply path, driven by `posthastectl`.
- [Scripting security & threat model](https://posthaste.theor.net/docs/scripting-security): the trust relationships and mitigations for event-triggered code (`watch --exec`, `exec`/`webhook` rules).
- [User guide](https://posthaste.theor.net/docs): the walkthrough-style user/operator guide.

## Decision records and design history

The durable specs above record *what is true now*. The reasoning, deviations,
and forward plans behind them live as dated records in [`eph/`](eph/INDEX.md) — RFCs
(the architecture-cleanup, scripting, drafts, provider-reliability, lifecycle,
and client-resilience programs), audits, design notes, and the reality ledger
([DEVIATION-L2-architecture-cleanup.md](eph/DEVIATION-L2-architecture-cleanup.md)).
The architecture-cleanup refactor these specs describe has **landed** (M0–M9c);
its RFC is [RFC-L2-architecture-cleanup.md](eph/RFC-L2-architecture-cleanup.md).
[Release channels](eph/DESIGN-L2-release-channels.md) — nightly (dogfood/devtools)
versus stable builds, updater manifests, and signing gates — is a design note in
that corpus. Open technical debt tracked here: [issues/L2-runtime-lifecycle-debt.md](issues/L2-runtime-lifecycle-debt.md).
