---
title: Posthaste
description: Your mail, delivered at Posthaste
modified: 2026-06-24
reviewed: 2026-06-24
---

# Posthaste specs

Current rewritten specs live in domain directories:

- [Mail state](state/mail/L1.md): canonical mail state, derived projections, query evaluation, and conversation freshness.
- [Runtime](runtime/L1.md): UI-facing runtime contract for the bundled application, embedded authority runtime, and future deployment adapters.
- [Replication](replication/L1.md): coherent links — the optimistic up-channel, authoritative down-channel, and confirmation-watermark convergence that move state between client, runtime, and backend. Two seams have their own sub-domains: the [client↔runtime link](replication/client-link/L1.md) (the device replica) and the [runtime↔backend link](replication/backend-link/L1.md) (the BackendApi seam).
- [API](api/L1.md): external `/v1` HTTP and SSE contract over those projections.
- [Backend](backend/L1.md): service, store, provider, runtime, event, and API implementation boundaries.
- [Client](client/L1.md): renderer contract over runtime state, presentation state, subscriptions, and actions.

Legacy specs that have not been rewritten yet live under [stale specs](stale/L0-api.md).
