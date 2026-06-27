---
title: Posthaste
description: Your mail, delivered at Posthaste
modified: 2026-06-26
reviewed: 2026-06-26
---

# Posthaste specs

Current rewritten specs live in domain directories:

- [Mail state](state/mail/L1.md): canonical mail state, derived projections, query evaluation, and conversation freshness.
- [Runtime](runtime/L1.md): UI-facing runtime contract for the bundled application, embedded authority runtime, and future deployment adapters.
- [Replication](replication/L1.md): coherent links — the optimistic up-channel, authoritative down-channel, and confirmation-watermark convergence that move state between client, runtime, and backend. Two seams have their own sub-domains: the [client↔runtime link](replication/client-link/L1.md) (the device replica) and the [runtime↔backend link](replication/backend-link/L1.md) (the BackendApi seam).
- [API](api/L1.md): external `/v1` HTTP and SSE contract over those projections.
- [Backend](backend/L1.md): service, store, provider, runtime, event, and API implementation boundaries.
- [Testing](testing/L0.md): behavior-contract coverage model, the shared `posthaste-testkit` harness and `StalwartFixture`, and the verification ladder. The settlement recorder, runtime-in-harness, and declarative TOML fixtures have landed; the remaining forward contract (`posthastectl`) is in the [testkit roadmap](eph/PLAN-L2-testkit-roadmap.md).
- [Release channels](eph/DESIGN-L2-release-channels.md): nightly (dogfood/devtools) versus stable (public beta/release) builds, updater manifests, and signing gates.
