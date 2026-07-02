---
title: Posthaste
description: Your mail, delivered at Posthaste
modified: 2026-07-02
reviewed: 2026-07-02
---

# Posthaste specs

Current rewritten specs live in domain directories:

- [Mail state](state/mail/L1.md): canonical mail state, derived projections, query evaluation, and conversation freshness.
- [Runtime](runtime/L1.md): UI-facing runtime contract for the bundled application, embedded authority server, and future deployment adapters.
- [Replication](replication/L1.md): coherent links — the optimistic up-channel, authoritative down-channel, and confirmation-watermark convergence that move state between client, runtime, and authority server. Two seams have their own sub-domains: the [client↔runtime link](replication/client-link/L1.md) (the device replica) and the [runtime↔authority-server link](replication/authority-server-link/L1.md) (the AuthorityServerLink seam).
- [API](api/L1.md): external `/v1` HTTP and SSE contract over those projections.
- [Authority server](authority-server/L1.md): the far node's service, store, provider, account-runtime, event, and API implementation boundaries.
- [Crate topology](architecture/L2-crate-topology.md): the workspace crate set, ownership, dependency hierarchy, and the wasm-pure frontier.
- [UI](ui/L0.md): the mail shell's navigation model — view kinds, pane focus, and the keyboard contract. Partial: only the navigation model (L0) and keyboard shortcuts (L1) are authored; the remaining UI sections are a follow-up.
- [Testing](testing/L0.md): behavior-contract coverage model, the shared `posthaste-testkit` harness and `StalwartFixture`, and the verification ladder. The settlement recorder, runtime-in-harness, and declarative TOML fixtures have landed; the remaining forward contract (`posthastectl`) is in the [testkit roadmap](eph/PLAN-L2-testkit-roadmap.md).
- [Release channels](eph/DESIGN-L2-release-channels.md): nightly (dogfood/devtools) versus stable (public beta/release) builds, updater manifests, and signing gates.

An architecture-cleanup refactor is in progress: the drain/outbox is
[eph/RFC-L2-architecture-cleanup.md](eph/RFC-L2-architecture-cleanup.md), the
reality ledger is [eph/DEVIATION-L2-architecture-cleanup.md](eph/DEVIATION-L2-architecture-cleanup.md).
The superseded (pre-refactor) spec tree lives in the main tree's `docs/`
until this workspace merges; open issues live in the main tree's
`docs/issues/`, plus [issues/L2-runtime-lifecycle-debt.md](issues/L2-runtime-lifecycle-debt.md)
opened here.
