---
scope: L2
summary: "Phase 2 contract for the server-authoritative undo/redo log: a per-account rev_log store table + cursor + a RevLog synced view, with the client proposing idempotent cursor moves the server arbitrates (last-writer-wins + sync re-convergence). Promotes the Phase 1 client-local cursor to synced account state for cross-device undo."
modified: 2026-06-28
reviewed: 2026-06-28
lifecycle: ephemeral
type: DESIGN
status: "Slices 1–5c done: store tables, RevLog view, forward-action append + cursor auto-advance, revCursor arbitration, client send (revCursor) + receive (mirror), + multi-account per-store history with global Ctrl+Z merge by createdAt. Remaining: JMAP per-message version; e2e verification against the real dev stack."
depends:
  - path: docs/eph/DESIGN-L2-undo-redo-synced-history
    note: "the overview + Phase 1 (shipped .26); this is the Phase 2 implementable contract"
  - path: docs/runtime/mutations/L1
    section: "1. Mutation pipeline and catalog"
  - path: docs/eph/DESIGN-L2-client-link-reactive-store
  - path: docs/eph/DESIGN-L2-local-first-mutations
dependents: []
---

# The RevLog: a server-authoritative synced undo/redo log

> **Status: REFERENCE (design contract) — largely realized.** The Phase 2
> implementable contract. Slices 1–5c landed (store tables, RevLog synced view,
> forward-action append + cursor auto-advance, revCursor arbitration, client
> send/receive mirror, multi-account per-store history with global Ctrl+Z merge);
> see the `feat(runtime): Phase 2 Slice …` / `feat(web): Phase 2 Slice …`
> commits. **Remaining:** JMAP per-message version; e2e verification against the
> real dev stack (per the `status:` field in the frontmatter below).

This is the **Phase 2 implementable contract** for the synced-history refactor.
Phase 1 (a client-local optimistic cursor, durable in IndexedDB, round-trip-free)
shipped in `.26` — it moved history ownership to the client + retired the runtime's
seq-keyed stacks. Phase 2 promotes the log to **synced account state** so undo/redo
works cross-device: undo on the phone what was done on the desktop. The client
cursor stays optimistic (Phase 1 behavior preserved); the log becomes
server-authoritative.

See [`DESIGN-L2-undo-redo-synced-history`](DESIGN-L2-undo-redo-synced-history.md) for
the problem, the model, + the Phase 1 status. This doc defines the store, the
synced view, the arbitration protocol, + the migration.

## Authoritative state (the store)

Two per-account tables in `crates/posthaste-store`:

**`rev_log`** — the append-only reversible-operation log:

| column       | type        | notes                                                          |
| ------------ | ----------- | -------------------------------------------------------------- |
| `account_id` | account id  | partition key                                                  |
| `step_id`    | ULID (text) | globally-orderable id (Phase 1's `RevStep.id`); the cursor key |
| `seq`        | u32         | per-account monotonic append order (the sync delta cursor)     |
| `message_id` | message id  | the message the step mutates                                   |
| `source_id`  | source id   | the account/source the message lives under                     |
| `diff`       | JSON        | `MessageChangeDiff` (`{keywords,mailboxes}{added,removed}`)    |
| `created_at` | timestamp   | for tie-break/display only; `step_id`/`seq` are the order      |

**`rev_cursor`** — the per-account cursor (one row):

| column           | type            | notes                                                       |
| ---------------- | --------------- | ----------------------------------------------------------- |
| `account_id`     | account id      | partition key                                               |
| `cursor_step_id` | ULID (nullable) | the topmost APPLIED step; `null` = all undone (cursor = -1) |
| `redo_tail`      | JSON ULID[]     | the undone steps above the cursor, in `seq` order           |

The log is append-only (forward actions append; nothing mutates a row). The cursor
is mutable (undo/redo move it). `step_id` is the logical key the client navigates
(Phase 1 already uses it); `seq` is the per-account append order the sync delta
cursors over.

## The RevLog synced view

`ViewKind::RevLog` mirrors the log + cursor to every device via the existing
delta pipeline (`crates/posthaste-runtime/src/views.rs`):

- **Full snapshot** on open / reconnect: the current `rev_log` rows (capped, see
  below) + the `rev_cursor` row.
- **Append delta**: a new `rev_log` row (forward action confirmed) — `{step_id,
seq, message_id, source_id, diff, created_at}`. Carries `seq` so the client
  detects gaps + re-snapshots.
- **Cursor delta**: a `rev_cursor` update — `{cursor_step_id, redo_tail}`.

The client's Phase 1 `IndexedDbUndoHistoryStore` becomes the **mirror** of this
view: it holds the same `RevStep[]` + cursor (the shape is unchanged — Phase 1
shaped it for this). The mirror persists alongside the outbox (offline undo works
against the mirror; the cursor assignment queues in the outbox + converges on
reconnect, idempotent).

## The protocol (client proposes, server arbitrates)

**Forward action** (e.g. archive, flag) — the diff is captured client-side
(Phase 1's `captureMutationDiffJson`) + the step is appended on confirmation:

1. Client accepts the optimism (outbox) + captures the diff.
2. Client dispatches the mutation (the normal pipeline).
3. On `Confirmed`, the server appends a `rev_log` row (server-assigned `seq`;
   client-supplied `step_id`/`diff`) + broadcasts an append delta.
4. The client mirror adds the step + (if no undo intervened) advances its cursor.

The `step_id` is client-supplied (the client knows it before confirmation — it
can navigate optimistically). The server validates uniqueness per account
(re-delivery of the same forward action is idempotent on `step_id`).

**Undo / redo** — the client moves its optimistic cursor locally (instant, Phase
1 unchanged) + sends an idempotent cursor assignment:

- Undo: `cursor := pred(cursor_step_id)` — move to the previous step.
- Redo: `cursor := redo_tail.head` (pop the redo tail).
- A forward action after undo: truncates the redo tail + appends (the cursor
  moves to the new step).

The assignment is `{account_id, cursor_step_id, redo_tail}`. The server:

1. Validates `cursor_step_id` exists in `rev_log` (or is `null`) + the redo tail
   steps exist.
2. Applies it to `rev_cursor` (idempotent — re-applying the same assignment is a
   no-op) + broadcasts a cursor delta.

Idempotent + id-keyed: re-delivery, reorder, + offline replay are all safe (the
cursor is a pure assignment, not an increment). This is why Phase 1 used `step_id`
not session seqs.

## Concurrency: last-writer-wins + re-convergence

Two devices undo simultaneously → both send cursor assignments. The server applies
them in **arrival order**; the idempotent assignment tolerates reordering. The
device that lost the race re-converges via the synced `RevLog` view: its
optimistic cursor drifts briefly until the cursor delta corrects it (the same
optimism-then-settle pattern the mail list uses).

Chosen over expected-cursor CAS because undo is a user action (low true-concurrency),
the idempotent assignment + sync re-convergence keep it simple, + this preserves
Phase 1's round-trip-free optimism (no retry on the hot path). CAS would add
round-trips + retry logic for a rare race.

## Truncation / cap

Per-account `MAX_HISTORY = 50` (carried from Phase 1). When the log exceeds the
cap, drop the **oldest** `rev_log` rows from the undoable tail (below the
earliest step still reachable from the cursor or redo tail). All devices agree
(the authoritative log is bounded; the eviction broadcasts as append-deltas'
inverse — a `rev_log` row removal). Evicted steps can no longer be undone.

## Migration from Phase 1

- The client `IndexedDbUndoHistoryStore` becomes the `RevLog` view mirror (the
  `RevStep[]` + cursor shape is unchanged). The `.27` schema-shared IndexedDB
  fix (v2 creates both `outbox` + `undoHistory`) is the durable base.
- The runtime stays **history-free** (Phase 1b retirement holds — no
  `undo_history`/`redo_history`/`MutationHistory` frame).
- Forward-action diff capture stays client-side (`captureMutationDiffJson`); the
  server appends the supplied diff on confirmation.
- The undo/redo vehicle stays `message.applyDiff` (Phase 1) — the cursor move is
  a separate `revCursor` control mutation, not a mail mutation.

## Resolved open questions

Carried from the overview doc; resolved here:

- **Global vs per-account ordering** → per-account (devices have different account
  sets; clean for cross-device).
- **Redo-tail durability** → sync the cursor explicitly (`cursor_step_id` +
  `redo_tail` are part of the `RevLog` view); no client re-derivation.
- **Cap/eviction** → per-account, count-based (`MAX_HISTORY = 50`), oldest-first.
- **Non-message reversibility** → out of scope (messages only; revisit if users
  expect undo on settings/mailboxes).

## Out of scope

- Non-message reversibility (settings, account, smart-mailbox actions).
- JMAP/IMAP provider-level undo (the `RevLog` is Posthaste account state, not
  synced to the provider).

Global cross-account undo is IN scope (Slice 5c): the client opens a per-account
`RevLog` view for every enabled account + the global Ctrl+Z merges the
per-account partitions by `createdAt` (the latest undoable step across all
accounts) — no globally-ordered log needed.

## Code anchors (Phase 2 targets)

- Store tables: `crates/posthaste-store` (`rev_log`, `rev_cursor`, the SQL
  migrations + query functions).
- Synced view: `crates/posthaste-runtime/src/views.rs` (`ViewKind::RevLog`,
  `build_snapshot`, the delta pipeline), `DESIGN-L2-client-link-reactive-store`.
- Cursor arbitration: a new `revCursor` runtime mutation (the
  `{cursor_step_id, redo_tail}` assignment) in `runtime/build.rs` +
  `runtime-contract`.
- Client mirror: `apps/web/src/runtime/replica/undoHistoryStore.ts`
  (`reconcileWithServer` adopts the server's snapshot with an optimism guard —
  a local move stays optimistic until the server echoes the cursor or the
  `OPTIMISM_TIMEOUT_MS` safety valve converges), `apps/web/src/hooks/useRevLogMirror.ts`
  (subscribes to the `RevLog` view + reconciles the store — the RECEIVE half of
  cross-device cursor sync). DONE (Slice 5a sent the `revCursor`; Slice 5b
  mirrors it). The store holds per-account partitions (one `RevStep[]` + cursor
  per account — the Phase 1 singleton mixing is gone); `MailClient.tsx` renders
  `<RevLogMirrors accountIds={enabledAccounts} />` to mirror every enabled
  account. DONE (Slice 5c).
- Reuse (unchanged): `MessageChangeDiff`/`inverse`/`from_before_after`
  (`crates/posthaste-link-core/src/message.rs`), `captureMutationDiffJson`
  (`posthaste-link-wasm`), the outbox, `message.applyDiff`.
