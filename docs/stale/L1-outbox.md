---
scope: L1
summary: "Local-first command outbox: operation envelope, lifecycle state machine, conflict policy, temp-id reconciliation, settlement"
modified: 2026-06-21
reviewed: 2026-06-21
depends:
  - path: docs/L1-sync
  - path: docs/L1-api
  - path: docs/L0-api
dependents: []
---

# Command outbox domain -- L1

Mutations are **local-first**: a command is applied to local state immediately,
persisted as an operation, and flushed to the next tier when it is reachable.
This replaces the previous provider-first ordering, where a mutation required the
provider round-trip to succeed before any local change and failed (with an
optimistic UI rollback) when offline.

The outbox is a **two-tier, opaque** abstraction:

- **Tier 1 — client <-> runtime (TypeScript).** When the client cannot reach the
  runtime, commands are queued in a durable client outbox and flushed on
  reconnect; the client renders best-effort from what it has.
- **Tier 2 — runtime <-> provider (Rust).** When the runtime cannot reach the
  provider, commands are queued in the durable store outbox (`outbox_operation`)
  after being applied locally, and flushed to the provider on reconnect.

Both tiers conform to the **same operation envelope and state machine**, defined
once in `posthaste-domain` and regenerated into the client via the OpenAPI
schema, so the two implementations cannot drift. Dispatch is opaque: callers
invoke the same command methods and the dispatcher decides send-now vs enqueue.

## Operation model

An operation is the unit persisted and flushed at every tier
(`posthaste_domain::Operation`):

- `id` — client-minted, globally stable; the idempotency key across all tiers.
- `accountId`.
- `entity` — `{ kind: message | draft, id }`. `id` may be a client-minted
  temporary id until reconciled (see [temp-id reconciliation](#temp-id-reconciliation)).
- `kind` — one of `setKeywords`, `replaceMailboxes`, `destroy`, `draftCreate`,
  `draftUpdate`, `draftDelete`, `send`.
- `payload` — kind-specific JSON (the wrapped command or draft body), keeping the
  envelope uniform across kinds and tiers.
- `baseCursor` — optimistic-concurrency token captured at enqueue; `None` for
  entity-creating ops.
- `state` — see [state machine](#state-machine).
- `attempts`, `lastError`.
- `dependsOn` — the operation that must settle first, preserving per-entity
  ordering (a `draftUpdate` never flushes before the `draftCreate` it builds on).
- `createdAt`, `updatedAt`.

The Tier-2 store persists operations in `outbox_operation`, ordered by insertion
(`rowid`).

### Idempotency

`id` is the idempotency key. Enqueue is idempotent: re-enqueuing an existing id
is a no-op that returns the stored operation. A tier must never apply the same
id twice — neither the client re-flushing to the runtime, nor the runtime
re-pushing to the provider. Provider protocols (IMAP, JMAP) are not assumed
idempotent, so each tier records what it has applied/pushed by `id` rather than
relying on the next tier to dedupe.

## State machine

```text
pending ──▶ inflight ──▶ applied
   ▲           │  │  └──▶ failed
   └───────────┘  └─────▶ conflicted ──▶ inflight (after resolution)
```

- `pending` — persisted locally, applied optimistically, not yet sent onward.
- `inflight` — currently being flushed to the next tier.
- `applied` — accepted by the next tier; awaiting prune (terminal).
- `conflicted` — base version diverged at the next tier; needs resolution.
- `failed` — permanent failure (e.g. validation); surfaced to the user (terminal).

Flushable states are `pending` and `conflicted`. Terminal states are `applied`
and `failed`. `OperationState::can_transition_to` encodes the allowed matrix and
is mirrored on the client.

## Conflict policy

Resolved per op kind (`OperationKind::conflict_policy`), encoded in the shared
model so both tiers agree:

- **`LocalWins`** — the local edit overwrites the next tier's state. Used by
  `draftCreate` / `draftUpdate` / `draftDelete` / `send`: the author's
  in-progress edit is authoritative.
- **`RefreshAndKeep`** — refresh from the next tier, keep the optimistic value,
  and surface only on true divergence. Used by `setKeywords` /
  `replaceMailboxes` / `destroy`; mirrors the existing `StateMismatch` refresh.

## Temp-id reconciliation

An entity created offline (only `draftCreate` creates an entity) has no provider
id. On the first successful flush the provider returns the real id, the runtime
emits the assignment in the operation settlement, and
`reconcile_operation_entity_id` rewrites every still-queued op for that account
from the temp id to the provider id (so dependent `draftUpdate` / `send` ops
target the right entity).

Queued-op reconciliation is not enough on its own: a draft can flush (and its
ops be pruned) between edits, after which the client would no longer know the
provider id. So drafts also use a **durable client-key alias**
(`draft_alias`): the client picks one stable `draftKey` per compose session and
sends it on every save. The runtime maps `draftKey -> current entity id` (the
temp id before the first flush, the provider id after) and updates the alias on
each draft settlement (`update_draft_alias_entity`). `save_draft` resolves the
key to the live entity id (creating on first use, updating thereafter), so
repeated edits update one provider draft instead of creating duplicates. The
alias is dropped on `delete_draft`.

## Settlement

When an operation reaches a terminal outcome it is propagated downstream via the
`operation.settled` event (`EVENT_TOPIC_OPERATION_SETTLED`) carrying an
`OperationSettlement`:

- `id`.
- `outcome` — `applied` | `conflicted` | `failed`.
- `assignedEntityId` — set when a temp entity id was reconciled on this flush.
- `error`.

The downstream tier (runtime -> client, ultimately the UI) uses settlement to
clear optimistic state, apply an id reassignment, or surface a conflict/failure.
