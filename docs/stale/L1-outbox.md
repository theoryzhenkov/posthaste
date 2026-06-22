---
scope: L1
summary: "Local-first command outbox: operation families, read-time overlay, convergence, temp-id reconciliation, settlement"
modified: 2026-06-22
reviewed: 2026-06-22
depends:
  - path: docs/L1-sync
  - path: docs/L1-api
  - path: docs/L0-api
dependents: []
---

# Command outbox domain -- L1

Mutations are **local-first**: a command is recorded as an operation and reflected
in reads immediately, then flushed to the provider when it is reachable. This
replaces the previous provider-first ordering, where a mutation required the
provider round-trip to succeed before any local change.

This document defines the **Tier-2** outbox: the boundary between the runtime and
the provider (IMAP/JMAP). A future Tier-1 outbox (client <-> runtime) is sketched
under [tiers](#tiers) but is intentionally **not** designed here — we build the
runtime/provider model for what mail actually needs now and unify later, from
evidence, rather than committing to a shared cross-tier envelope up front.

## Foundations

Three invariants carry the whole design. Everything else follows from them.

1. **The authoritative projection is written only by sync.** Optimistic mutations
   never write the message/mailbox projection in place. The provider (via the
   sync writer, see [docs/L1-sync](docs/L1-sync)) is the single writer of record.
2. **Pending operations are a read-time overlay.** A read returns the
   authoritative projection *folded with* the account's pending operations. The
   outbox **is** the overlay; there is no second optimistic copy of the
   projection to keep in step.
3. **Dropping an operation always converges.** When an operation leaves the
   outbox — applied, failed, or abandoned — the read model reverts to provider
   truth with no residual divergence. Applied changes are then carried by the
   next sync; failed/abandoned changes simply disappear from reads.

The previous design violated all three: it wrote optimistic changes into the
authoritative projection, never advanced the cursor, and parked unresolved
operations in a `conflicted` state forever — so an abandoned operation left the
projection permanently diverged for any object incremental sync would not retouch,
and a single divergence wedged every later operation on the same entity.

## Operation families

The unit persisted and flushed is an `Operation` (see [model](#operation-model)),
but operations come in three families with **distinct semantics and lifecycles**.
The family is determined by `kind`. There is no generic per-operation
concurrency token and no generic conflict policy; each family states its own rule.

### State assertions — `setKeywords`, `replaceMailboxes`, `destroy`

These express an **idempotent desired state** for a message, not a delta against a
base version. "Membership is `{Archive}`", "`$flagged` is set", "this message is
destroyed". Consequences:

- **No per-operation concurrency base.** They flush without an `ifInState`
  guard — last-writer-wins against current provider state. Mail providers are
  built for this: a JMAP `ifInState` mismatch means "account state moved", not
  "this assertion is unsafe". If the provider rejects on state, the runtime
  **refetches state and re-applies the same desired state** (bounded retries),
  never parking a conflict.
- **They coalesce.** Enqueuing an assertion supersedes the pending assertion it
  replaces: a new `replaceMailboxes` replaces the pending `replaceMailboxes`, a
  new `setKeywords` merges its add/remove deltas into the pending `setKeywords`
  (the newer delta wins on conflicts), and a `destroy` supersedes every pending
  assertion for that entity. Only still-`pending` ops are coalesced; an inflight
  op is left alone. This collapses ordering for the common case — no dependency
  chain is needed between independent assertions.
- **Terminal outcomes are `applied` or `failed`.** A hard reject (entity gone,
  permission denied, mailbox missing) fails terminally and is surfaced; on drop,
  the read reverts to provider truth.

### Draft content — `draftCreate`, `draftUpdate`, `draftDelete`

These carry content for an entity with identity, resolved **LocalWins**: the
author's in-progress edit is authoritative and overwrites provider state.

- A new draft has no provider id; on first successful flush the provider assigns
  one and the runtime reconciles it (see
  [temp-id reconciliation](#temp-id-reconciliation)).
- Draft operations for one entity flush in **enqueue order** via `dependsOn`
  (`draftUpdate` never flushes before the `draftCreate` it builds on). If a
  predecessor fails terminally, its dependents are **cancelled**, not retried.

### Send — `send`

Composing and sending a message enqueues a `send` operation (a unique entity id
per send, so it never coalesces) and flushes it on the next connectivity window;
no live gateway is required to accept the send. One-shot and terminal. A send
flushes **at most once per idempotency id**:
provider send is not assumed idempotent. A `send` discovered already `inflight`
at drain time was interrupted after a prior flush began and may have already
reached the provider, so it is **failed terminally rather than resent**, and the
user is surfaced the unknown outcome. Outcomes are `applied` or `failed`.

## Read-time overlay

Reads fold the pending-operation set onto the authoritative projection:

- **Message read / list**: apply pending assertions for each visible message —
  keyword sets, mailbox membership, and `destroy` tombstones (a destroyed message
  is hidden from every read until the operation drops). Pending `draftCreate`
  operations surface as messages in the Drafts mailbox.
- **Sidebar mailbox counts** are folded by applying the per-message delta between
  a message's base and overlaid membership/read state to the stored counts. The
  delta is bounded by the messages with pending assertions and is skipped
  entirely when the outbox is empty.
- **Smart-mailbox counts** fold by counting the overlaid rule result when any
  pending message assertion exists, and fall back to the stored SQL count
  otherwise, so a message archived offline leaves the Inbox count immediately and
  reappears only if the operation fails.

The fold is pure and derived; it never mutates stored projection rows. A
materialized overlay cache is permitted as an internal optimization (L2/L3) but
must produce identical results to folding the live operation set.

## Operation model

An operation (`posthaste_domain::Operation`):

- `id` — runtime-minted, globally stable; the idempotency key. A tier must never
  apply the same `id` twice.
- `accountId`.
- `entity` — `{ kind: message | draft, id }`. `id` may be a temporary id until
  reconciled (see [temp-id reconciliation](#temp-id-reconciliation)).
- `kind` — `setKeywords` | `replaceMailboxes` | `destroy` | `draftCreate` |
  `draftUpdate` | `draftDelete` | `send`.
- `payload` — kind-specific JSON (the wrapped command or draft body).
- `state` — see [state machine](#state-machine).
- `attempts`, `lastError`.
- `dependsOn` — predecessor that must settle first; set **only** for draft
  chains. State assertions coalesce instead of depending, and carry `None`.
- `createdAt`, `updatedAt`.

There is **no `baseCursor` field**. Optimistic concurrency against a per-operation
base was the wrong model for mail and is removed; convergence is owned by sync.

The store persists operations in `outbox_operation`, ordered by insertion.

### Idempotency

`id` is the idempotency key. Enqueue is idempotent: re-enqueuing an existing `id`
returns the stored operation. The runtime records what it has pushed to the
provider by `id` and never re-pushes a settled operation.

## State machine

```text
pending ──▶ inflight ──▶ applied        (terminal)
   ▲           │     └──▶ failed         (terminal)
   └───────────┘  (transient: return to queue, retry next window)
```

- `pending` — recorded; reflected in reads via the overlay; not yet pushed.
- `inflight` — currently being pushed to the provider.
- `applied` — accepted by the provider; pruned. Terminal.
- `failed` — terminal failure (hard reject, or a cancelled draft dependent);
  surfaced to the user. Terminal.

There is **no resting `conflicted` state.** A provider state mismatch is resolved
*within* a flush attempt (refetch + re-apply for assertions; overwrite for
drafts), or it fails terminally. An operation never sits unresolved blocking the
queue.

A transient/offline failure returns the operation to `pending` and **stops the
drain** for that account so later operations retry together on the next
connectivity window. A persisted `inflight` row is also eligible for a later
flush: if the process exits after marking an operation inflight, the next drain
recovers it instead of wedging the outbox.

## Flush

The runtime drains an account's pending operations oldest-first when a gateway is
connected:

- **Ordering.** Draft dependents wait for their predecessor to settle. A
  cancelled predecessor cancels its dependents. State assertions have no
  dependencies (they coalesced at enqueue).
- **State assertions** push the desired state with no `ifInState` guard. On a
  provider state mismatch, refetch account state and re-apply, bounded; on hard
  reject, fail.
- **Drafts** overwrite provider state; first flush reconciles the temp id.
- **Send** pushes once per `id`.
- **Transient failure** (offline/auth) returns the operation to `pending` and
  stops draining.
- On apply, the operation is pruned and an [`operation.settled`](#settlement)
  event is emitted; the overlay entry disappears and the following sync carries
  the authoritative change.

Flushing is ordered relative to sync: the runtime **flushes before a pull sync**
so the provider is not re-read into the projection ahead of the local intent, and
**flushes again after sync** to drain operations enqueued while applying a batch.

## Temp-id reconciliation

Only `draftCreate` creates an entity. On first successful flush the provider
returns the real id; the runtime emits the assignment in the settlement and
rewrites every still-queued operation for that account from the temp id to the
provider id, so dependent `draftUpdate` / `send` operations target the right
entity.

Queued-op rewriting is not sufficient alone: a draft can flush (and its ops be
pruned) between edits, after which the temp id is gone. Drafts therefore also use
a durable **draft-key alias** (`draft_alias`): the client picks one stable
`draftKey` per compose session and sends it on every save. The runtime maps
`draftKey -> current entity id` (temp before first flush, provider id after) and
updates the alias on each draft settlement. `save_draft` resolves the key to the
live entity id — creating on first use, updating thereafter — so repeated edits
update one provider draft instead of creating duplicates. The alias is dropped on
`delete_draft`.

## Settlement

When an operation reaches a terminal outcome it is propagated via the
`operation.settled` event (`EVENT_TOPIC_OPERATION_SETTLED`) carrying an
`OperationSettlement`:

- `id`.
- `outcome` — `applied` | `failed`.
- `assignedEntityId` — set when a temp entity id was reconciled on this flush.
- `error` — set on `failed`.

Consumers use settlement to clear optimistic UI, apply an id reassignment, or
surface a failure. There is no `conflicted` outcome: assertions converge and
drafts overwrite, so the only terminal outcomes are applied and failed.

## Tiers

The implemented outbox is **Tier 2 (runtime <-> provider)**, defined above.

A **Tier 1 (client <-> runtime)** outbox — queuing commands in the web client
when the runtime is unreachable and flushing on reconnect — is anticipated but
**not specified here**. When it is built, any envelope or state-machine sharing
with Tier 2 will be extracted from the two concrete implementations rather than
imposed in advance.

## Assertions

| ID                  | Sev.   | Assertion                                                                                                                  |
| ------------------- | ------ | -------------------------------------------------------------------------------------------------------------------------- |
| projection-sync-only | MUST  | The authoritative message/mailbox projection is written only by the sync writer; optimistic mutations never write it in place. |
| overlay-fold        | MUST   | A read returns the authoritative projection folded with the account's pending operations.                                  |
| converge-on-drop    | MUST   | When an operation is pruned or fails, the read model reverts to provider truth with no residual divergence.                 |
| assertion-no-occ    | MUST   | `setKeywords`/`replaceMailboxes`/`destroy` flush without a per-operation concurrency base; a provider state mismatch triggers refetch-and-reapply, not a resting conflict. |
| assertion-coalesce  | SHOULD | Enqueuing a state assertion supersedes any pending assertion of the same kind on the same entity.                          |
| no-resting-conflict | MUST   | No operation rests in a conflicted state; a mismatch resolves within a flush attempt or fails terminally.                  |
| no-entity-wedge     | MUST   | A failed or retrying operation never permanently blocks unrelated operations or other entities' operations from flushing.   |
| draft-localwins     | MUST   | Draft operations overwrite provider state and reconcile temp ids to provider ids on first flush.                           |
| draft-key-alias     | MUST   | Repeated saves under one `draftKey` update a single provider draft, never creating duplicates.                             |
| chain-fail-cancel   | MUST   | If a draft-chain predecessor fails terminally, its dependents are cancelled rather than retried.                           |
| send-once           | MUST   | A `send` operation pushes to the provider at most once per idempotency id; an interrupted inflight send is failed, not resent. |
| counts-fold         | SHOULD | Sidebar and smart-mailbox counts reflect the read-time overlay while assertions are pending.                              |
| op-idempotent       | MUST   | Enqueue is idempotent on `id`; the runtime never re-pushes a settled operation.                                            |
| flush-stops-offline | SHOULD | A transient/offline failure returns the operation to `pending` and stops the account drain until the next window.          |
| inflight-recovers    | MUST   | A persisted `inflight` operation is eligible for a later drain so a crash mid-flush cannot wedge the outbox.              |
| flush-before-pull   | MUST   | Pending operations are flushed before a pull sync so the provider is not re-read ahead of local intent.                    |
