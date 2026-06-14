---
title: Posthaste
description: Your mail, delivered at Posthaste
modified: 2026-06-15
reviewed: 2026-06-15
---

# Posthaste specs

Current rewritten specs live in domain directories:

- [Mail state](state/mail/L1.md): canonical mail state, derived projections, query evaluation, and conversation freshness.
- [Runtime](runtime/L1.md): UI-facing runtime contract for the bundled application, embedded authority runtime, and future deployment adapters.
- [API](api/L1.md): external `/v1` HTTP and SSE contract over those projections.
- [Backend](backend/L1.md): service, store, provider, runtime, event, and API implementation boundaries.
- [Client](client/L1.md): renderer contract over runtime state, presentation state, subscriptions, and actions.

Legacy specs that have not been rewritten yet live under [stale specs](stale/L0-api.md).
