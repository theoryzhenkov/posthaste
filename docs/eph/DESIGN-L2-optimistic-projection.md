---
scope: L2
summary: "Redesign optimistic message state as one shared convergence contract (replay) with two materialization strategies behind a single trait — lazy replay-at-read (client) and eager write-through-to-SQL (runtime) — removing the O(all) read-time fold that the link-bus rework introduced. Establishes the canonical vocabulary and the parity invariant (canonical == replay(base, unsettled))."
modified: 2026-06-25
reviewed: 2026-06-25
lifecycle: ephemeral
type: DESIGN
depends:
  - path: docs/eph/PLAN-L1-special-state-convention
  - path: docs/replication/client-link/L1
  - path: docs/state/mail/L2
dependents: []
---

# Optimistic projection: one contract, two materializations

## 1. The problem

Optimism is implemented **twice** today, with two vocabularies:

- **Store level** (`posthaste-domain`): an `outbox_operation` log folded over
  *every* message on *every* read (`fold_message_overlay` →
  `apply_operations_to_summary`). A mail-list recompute therefore loads all
  messages, folds, filters, sorts in memory, and keeps 50 — **O(all messages)
  per event, per view** (measured ~360× the store write; see
  [[link-bus-perf-regression]]). This is the regression the link-bus rework
  introduced when it dropped the indexed SQL path.
- **Link level** (`posthaste-link-core`): `MessageReplica` holds a `base` + an
  ordered list of `pending` mutations and computes `replay(base, assertions)`
  on read. This is the *correct* convergence model, and the
  `one-replica-both-seams` invariant already says client and runtime must share
  it.

The store-level fold is a second, divergent implementation of `replay`. This
redesign collapses them onto the shared contract and removes the O(all) read.

## 2. The model

**One convergence contract, one fold.** `replay(base, assertions) -> outcome`
(`posthaste-link-core::replay_message`) is the *single* definition of "apply
optimism to a message." Everything folds through it; nothing reimplements it.

**Two materialization strategies behind one trait.** `replay` can be computed
when read or when written; the right choice depends on the substrate, and that
is the *one* deliberately-split seam:

- **Lazy projection** (client): hold `base` + `unsettled` in memory; compute
  `replay` on each read. Perfect for a tens-of-row window; the rendered state is
  a pure function of its inputs and cannot drift. This is `MessageReplica` today.
- **Materialized projection** (runtime): apply each assertion eagerly to the
  canonical SQLite row and persist the result; reads are a plain indexed query
  with **no fold**. Required because the runtime's store is large and
  SQL-indexed — lazy replay over it is the O(all) bug. The fold still runs, but
  once per write/settle, not per read.

Both satisfy the same trait (§4); the materialized impl's correctness is checked
against the lazy impl as **oracle**: `canonical == replay(base, unsettled)`.

## 3. The runtime write-through mechanics

- **Mutation** applies its assertion to the canonical row immediately and records
  a durable **operation** in the outbox (the provider-flush queue + the
  unsettled marker).
- **Sync guard:** `apply_sync_batch` **skips messages that are unsettled** (the
  message is owned by its in-flight operation); messages not in the outbox sync
  normally.
- **Settle:** the gateway issues the mutation *and* a read in one request (JMAP
  `Email/set`+`Email/get`), so the response always carries the **authoritative,
  current** message — success confirms, "failure" returns the unchanged row (a
  free revert), and it also picks up any external change. On settle the runtime
  overwrites the canonical row with `replay(authoritative, remaining unsettled
  ops for that message)` and removes the operation; the message then syncs
  normally again.
- **Connection drops** are covered by the durable outbox: the operation persists
  and re-sends on reconnect; the message stays unsettled-and-guarded until it
  settles.

## 4. Canonical vocabulary

This domain has six near-synonyms. We adopt **link-core's** set and retire the
domain's parallel terms. Use these exact words everywhere.

| Canonical | Means | Retire (old synonyms) |
| --- | --- | --- |
| **base** | the authoritative, provider/sync-owned message state | — |
| **assertion** | one optimistic state effect (`SetKeywords`/`ReplaceMailboxes`/`Destroy`) | — |
| **operation** | a durable, provider-flushable outbox entry carrying one assertion + flush metadata; one per mutation | — |
| **mutation** | the user-facing named action that produces an operation (`MutationRequest`, `run_mutation`) | — |
| **unsettled** / **settled** | an operation not yet (vs. now) resolved by the provider; unsettled = `Pending`/`Inflight`/`Applied`, settled = `Confirmed`/`Failed` | "pending" as an umbrella (it is a *specific* state) |
| **replay** | the pure fold `replay(base, assertions)` — the one definition of applying optimism | **overlay**, `fold_message_overlay`, `apply_operations_to_summary` |
| **projection** | the optimistic effective state = `replay(base, unsettled assertions)` | — |
| **settle** | resolve an unsettled operation against the provider outcome | — |

The hierarchy: a **mutation** creates a durable **operation** (in the outbox)
whose effect is an **assertion**; **replay** folds the unsettled assertions over
the **base** to yield the **projection**; **settle** retires the operation.

**Open naming choice (needs sign-off):** the shared trait and its two impls.
Proposal:
- trait `MessageProjection` — `apply(assertion)`, `settle(id, outcome)`,
  `update_base(...)`, `project(message_id)`.
- impls `ReplayProjection` (lazy, in-memory — wraps today's `MessageReplica`)
  and `StoreProjection` (eager, SQL-backed).

Alternatives considered: `OptimisticStore`/`ReplicaStore` (trait);
`InMemoryReplica`/`SqlReplica` (impls). Flagging because "replica" overloads the
near-node sense and the runtime co-located is authority+replica collapsed.

## 5. Invariants (to assert in tests)

- **parity:** the materialized canonical equals the lazy projection —
  `StoreProjection.read(m) == replay(base(m), unsettled(m))` — checked against
  `ReplayProjection` as oracle.
- **no double-apply:** once the runtime canonical includes its own optimism, the
  base it serves the client already reflects it; the client folds only *its own*
  unsettled assertions on top. Optimism is represented once per node.
- **sync safety:** a sync never overwrites an unsettled message; a settled
  message syncs normally.
- **settle completeness:** on settle, the canonical row reflects the provider
  authority folded with any still-unsettled ops for that message (no lost
  optimism when multiple ops are in flight).

## 6. Slices

- **S1 — gateway returns the message.** `MailGateway::{set_keywords,
  replace_mailboxes, destroy_message}` do `set`+`get`; `MutationOutcome` carries
  the resulting summary. Mock + live. No behavior change yet.
- **S2 — write-through + settle.** Mutation applies the assertion to canonical;
  settle overwrites from the gateway outcome (revert-on-failure falls out).
  Outbox becomes flush-queue + unsettled marker.
- **S3 — sync unsettled-guard.** `apply_sync_batch` skips unsettled messages
  (index `outbox_operation(account_id, entity_id)`).
- **S4 — drop the read-time fold.** Remove `fold_message_overlay` from the read
  path; reads become the indexed `query_message_page_by_rule`. **The perf win
  lands here** — harness before/after on the 5,000-message fixture.
- **S5 — share the contract.** Route the runtime's write-time materialization
  through `link-core::replay`; introduce the `MessageProjection` trait with the
  two impls; assert parity against the lazy oracle.

## 7. Risks

- The sync-guard is the one genuinely new checkpoint (sync was previously the
  *only* writer of canonical, so race-free by construction). Explicit tests.
- Multiple in-flight ops per message ⇒ settle must re-fold remaining unsettled
  ops, not blind-overwrite (§5 settle completeness).
- Blast radius: the canonical read query backs the API, smart mailboxes, and
  views — S4 needs parity tests so overlay-aware behavior matches the old fold.
