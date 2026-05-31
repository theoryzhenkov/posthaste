---
scope: L1
type: DESIGN
lifecycle: ephemeral
summary: "Client read-model authority rules and typed read-call bootstrap plan"
modified: 2026-05-31
reviewed: 2026-05-31
depends:
  - path: docs/L1-ui
    section: Data fetching
  - path: docs/L1-api
  - path: docs/L1-search
dependents: []
---

# DESIGN: Client read models

## Why

The old client bootstrap used `GET /sidebar`, a UI-shaped cross-account aggregate.
That avoided N per-account mailbox requests, but it created a drift risk: feature
code could read `sidebar` as if it were the mailbox/domain authority, while
mutations and domain events had to remember to invalidate both sidebar and domain
caches.

## Decision

**Domain-named read models are the authorities. Client-composed typed read calls
hydrate them; UI aggregates do not become parallel authorities.**

Authoritative query keys must be named by domain entity, not by UI surface:

| Domain entity | Authoritative client key |
|---|---|
| Accounts | `queryKeys.accounts`, `queryKeys.account(accountId)` |
| Source mailboxes | `queryKeys.mailboxes(accountId)` |
| Smart mailboxes | `queryKeys.smartMailboxes`, `queryKeys.smartMailbox(id)` |
| Tags / user keywords | `queryKeys.tags` |

`queryKeys.sidebar` is removed from the active client data flow. The client-owned
mail navigation bootstrap uses `queryKeys.mailNavigationRead` and `POST /read`.

## Typed read-call implementation

`POST /read` exposes typed domain read operations, not frontend presets:

- `Account/list`
- `Mailbox/list`
- `SmartMailbox/list`
- `Tag/list`

The web client owns the composed navigation operation in
`apps/web/src/mailboxNavigationReadModels.ts`:

1. Request accounts.
2. Request mailboxes with `accountIds: "#accounts.enabledIds"`.
3. Request smart mailboxes.
4. Request tags with `accountIds: "#accounts.enabledIds"`.
5. Hydrate domain caches with `queryClient.setQueryData`.

This preserves the one-round-trip bootstrap without making the backend aware of a
specific UI surface. Other clients can compose different read graphs from the same
operation vocabulary.

## Boundary guard

`apps/web/scripts/check-query-boundaries.ts` enforces that direct sidebar cache reads
remain absent from feature code. The allowlist is empty: `queryKeys.sidebar` and raw
`['sidebar']` keys must not appear in active frontend source.

## Assertions

| ID | Sev. | Assertion |
|---|---|---|
| domain-authority | MUST | Domain feature code reads domain-named React Query keys rather than UI-surface aggregate keys. |
| typed-read-bootstrap | MUST | Mail navigation bootstrap uses client-composed typed read calls rather than a backend UI preset. |
| aggregate-hydration | SHOULD | Bootstrap calls hydrate domain caches instead of requiring consumers to read duplicate aggregate state. |
| sidebar-boundary | MUST | Direct references to `queryKeys.sidebar` or raw `['sidebar']` keys fail the web check. |

## Related implementation

- `POST /v1/read` — typed read-call endpoint.
- `apps/web/src/mailboxNavigationReadModels.ts` — client-owned mail navigation read operation.
- `apps/web/src/queryKeys.ts` — current key registry.
- `apps/web/src/domainCache.ts` — centralized cache invalidation and mutation result handling.
- `apps/web/scripts/check-query-boundaries.ts` — sidebar boundary guard.
