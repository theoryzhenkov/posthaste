---
scope: L2
summary: "Successor to the seq-mirrored runtime-owned undo/redo: model the history as a per-account, durable, server-authoritative reversible-operation log with a cursor that syncs to every device as a view, while evaluation stays local-optimistic (fold inverse(diff)/diff into the outbox). Decouples WHAT to undo (synced log, mirrored whole so chained undo is instant) from APPLYING it (local optimism, no round trip), and makes undo/redo cross-device — undo a move on the desktop, restore it from the phone."
modified: 2026-06-28
reviewed: 2026-06-28
lifecycle: ephemeral
type: DESIGN
supersedes:
  - path: docs/eph/DESIGN-L2-reversible-undo-redo
    note: "carries forward its two open questions (chained-undo latency, history persistence) and adds cross-device sync"
depends:
  - path: docs/runtime/mutations/L1
    section: "1. Mutation pipeline and catalog"
  - path: docs/eph/DESIGN-L2-client-link-reactive-store
  - path: docs/eph/DESIGN-L2-local-first-mutations
dependents: []
---

# Undo/redo as a synced, durable reversible-operation log

> **Status: REFERENCE (design note) — realized.** The overview + Phase 1 model.
> Phase 1 (client-owned optimistic history + runtime retirement) shipped; Phase 2
> (the server-authoritative synced log) was specified in
> DESIGN-L2-undo-redo-revlog-contract and implemented through Phase 2 Slice 5c.
> Kept as the design narrative behind that shipped work.

## Problem

The shipped design (`DESIGN-L2-reversible-undo-redo`) made undo _execution_ a
normal optimistic `message.applyDiff` mutation — good. But it kept the **history**
runtime-owned, per-session, in-memory, and exposed only the **single top** of each
stack on a `mutationHistory` frame. Three consequences, two of which that doc
flagged as carried-forward open questions:

1. **Chained-undo latency (open question #1).** The client mirrors only
   `undoTop`/`redoTop`, so it cannot navigate to step _N−1_ until the runtime
   confirms step _N_ and re-broadcasts the next top. `useRuntimeUndoRedo` enforces
   this with a `busyRef` serialization loop: **N undos = N round trips.** The view
   update is already optimistic; it is the _navigation_ that is round-trip-bound.
2. **No persistence (open question #2).** The history lives in
   `StoredSession` (in-memory, per session). A reload or new connection starts
   empty; `collapse_session_frames` does not even re-emit the history on
   reconnect, so `canUndo`/`canRedo` can go stale until the next mutation.
3. **Single-device.** History is private to one session, so you cannot undo on one
   device what you did on another.

The seq-keyed mirror is also **racy**: a new user action clears the redo stack and
re-broadcasts, but the client's `redoTopRef` is briefly stale; a redo in that
window sends a dead `redoOf` seq, which `run_apply_diff` still _applies_ (the diff
re-folds) without navigating — a phantom redo.

## Phase 1 status (shipped .26, 2026-06-28)

Consequences #1 + #2 are resolved; #3 (cross-device) remains for Phase 2.

- #1 (latency) — resolved: the round-trip-free `useUndoRedo` hook navigates the
  client-owned cursor locally (no `busyRef`); N undos dispatch immediately.
- #2 (persistence) — resolved: the `IndexedDB` `undoHistoryStore` persists the
  history alongside the outbox. **Caveat:** a version-skew bug shipped in .26 —
  the v2 upgrade creates only the `undoHistory` store, so `outboxStore`'s v1
  open downgrades + rejects, hanging the mail list; the fix is in flight as .27
  (coordinate the shared schema: the v2 upgrade creates _both_ `outbox` +
  `undoHistory`, and `outboxStore` opens at v2). Regression guard:
  `apps/web/e2e/idb-version-conflict.mjs`.
- #3 (cross-device) — unchanged: Phase 2 (server-authoritative synced log).

## Requirement (new)

Undo/redo must be **evaluated locally** (instant, optimistic, offline-capable) yet
the **history must be shared across a user's devices**: move a draft to Trash on
the desktop, pick up the phone, and restore it there. The history is therefore not
client-private state — it is **account state that syncs**.

## The model

Separate the two concerns the old design conflated:

- **WHAT to undo** — a per-account, ordered, durable **reversible-operation log**
  with a **cursor**. This is _authoritative server/store state_ that **syncs to
  every device as a view** (snapshot + deltas), exactly like a mail-list. Each
  client mirrors the **whole log**, so navigating any number of steps is a local
  read — no per-step round trip.
- **APPLYING it** — unchanged: fold `inverse(diff)` (undo) or `diff` (redo) into
  the outbox as a `message.applyDiff`, projected immediately. Instant and
  offline-capable; the existing convergence guard settles it.

The history stops being a session-private mirror and becomes a synced projection.
Evaluation stays at the edge; authority for _ordering and truncation_ moves to the
account's durable log so all devices agree.

### Data model

```text
RevStep {
  id:         StepId,        // stable, globally orderable (ULID); NOT a session seq
  account_id: AccountId,
  message_id: String,
  source_id:  String,
  diff:       MessageChangeDiff,   // reuse as-is: invertible, idempotent, state-free
  origin:     DeviceId,      // for display ("trashed on iPhone") + tie-breaks
  created_at: Timestamp,
}

RevLog {                     // per account, durable in posthaste-store
  steps:  Vec<RevStep>,      // append-ordered
  cursor: Option<StepId>,    // id of the topmost APPLIED step; None = all undone
}
```

`cursor` partitions the log: steps `<= cursor` are applied (undoable, newest
first); steps `> cursor` are undone (redoable, oldest first). The diff type is
reused verbatim — `inverse()` swaps added↔removed, `apply` is set-semantics
idempotent, both are state-free (link-core `message.rs`).

### Operations (idempotent, cursor keyed by step id)

Cursor moves are **absolute assignments keyed by step id**, never relative
"back-one" — so re-delivery and concurrent devices converge.

| Op                              | Log/cursor effect                                                          | Local evaluation                               |
| ------------------------------- | -------------------------------------------------------------------------- | ---------------------------------------------- |
| **forward** reversible mutation | append `S` after `cursor`, **truncate** the redoable tail, `cursor = S.id` | (the user action itself; its diff is captured) |
| **undo** of `S_k`               | `cursor := pred(S_k)` (the step before `S_k`, or `None`)                   | fold `inverse(S_k.diff)` into outbox           |
| **redo** to `S_j`               | `cursor := S_j.id` (the next redoable)                                     | fold `S_j.diff` into outbox                    |

Idempotency falls out of absolute assignment: undoing `S_k` twice is `cursor :=
pred(S_k)` twice — same result. A forward action truncates the tail (classic
redo-clear) authoritatively in the log.

## Sync and concurrency

The log syncs as a **view** (`ViewKind::RevLog { account_id }`) over the existing
reactive store + delta pipeline — no new transport. The server/store is the
**authority** for append, cursor, and truncation; clients mirror and propose.

A client undo/redo emits two coupled things through the normal mutation path:
the `message.applyDiff` (folds optimistically) **and** the proposed cursor
assignment. The server validates the assignment against the authoritative cursor,
persists, and broadcasts a log delta; other devices converge.

Concurrency resolution (authority = the store; assignments idempotent + monotone):

| Race                                                          | Resolution                                                                                                                                                                                                            |
| ------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Two devices undo the same top `S_k`                           | First sets `cursor := pred(S_k)`; the second arrives, sees `S_k` already undone, and is a **no-op**. Both local folds are idempotent, so views agree.                                                                 |
| Device A appends a new action while Device B has redo pending | A's forward **truncates** B's redoable tail. B's redo references a now-absent step → server **rejects** → B rolls back the optimistic fold via the existing outbox-rejection path, and the synced log greys redo out. |
| Undo races an unrelated edit to the same message              | Last-writer-wins on the message (unchanged); the diff's delta semantics stay correct (it removes exactly what it added).                                                                                              |

This replaces the old design's _racy client mirror_ with _authoritative
convergence_ — the stale-seq phantom-redo cannot occur because navigation is an
idempotent id-keyed assignment the server arbitrates, not a fire-and-hope seq.

## Persistence (two tiers)

- **Authoritative**: `RevLog` per account in `posthaste-store` (durable, survives
  restart, the cross-device source of truth).
- **Client mirror**: durable **alongside the outbox** (IndexedDB), so the log +
  cursor are present instantly on load, survive reload, and support **offline
  undo** (fold locally now; the cursor assignment + applyDiff flush from the
  outbox on reconnect, then converge with the authoritative log).

"Persist with the outbox" = the mirror shares the outbox's durability and flush
lifecycle; the authoritative copy is the new per-account store table.

## Cross-device walkthrough (the draft example)

1. **Desktop** moves a draft to Trash: `message.moveToRole{trash}` →
   invertible diff `{mailboxes:{added:[trash],removed:[drafts]}}`. The runtime
   appends `RevStep S` to the account's `RevLog`, `cursor = S`, persists, and
   broadcasts a log delta.
2. **Phone** (same account) has the `RevLog` view open; it receives the delta and
   now shows `canUndo` with top = "draft → Trash (on Desktop)".
3. **Phone** undoes: folds `inverse(S.diff)` (Trash → Drafts) into its outbox →
   the draft reappears in Drafts **instantly**; proposes `cursor := pred(S)`.
4. Server validates, sets the cursor, broadcasts. **Desktop** converges: the draft
   is back in Drafts, `canRedo` now shows "redo: draft → Trash".

This works precisely because trashing is a **move** (reversible diff). Permanent
`message.destroy` and `draft.discard` stay non-invertible and out of scope (same
as today) — "restore a trashed draft" is a move, so it is covered.

## What changes / what is retired

- **Retired (Phase 1b)** the seq-keyed per-session stacks (`undo_history`/`redo_history`,
  `pop_*_by_seq`, `push_undo/redo`, `remove_by_seq`, `next_seq`-for-history) and
  the `MutationHistory` single-top frame.
- **Retired (Phase 1a)** the hook's `busyRef`/queue serialization — navigation is
  local over the mirrored log; chained undo is instant.
- **Replaced (Phase 1b)** `undoOf`/`redoOf` seq hints — the client-owned cursor
  navigates locally; `applyDiff` is the undo/redo vehicle (no seq hint).
- **Add (Phase 2, pending)** the per-account `RevLog` store table +
  `ViewKind::RevLog` synced view + client mirror persisted with the outbox.
- **Reuse** unchanged: `MessageChangeDiff`/`inverse`/`from_before_after`, the
  outbox, the convergence guard, the reactive-store view/delta pipeline.

### Spec deltas (flag, do not silently diverge)

- `docs/runtime/mutations/L1` §5 — **realized (Phase 1b):** the `message.applyDiff`
  row dropped `undoOf`/`redoOf`; it is now the client-owned undo/redo vehicle
  (the hook dispatches `applyDiff(inverse(diff))` / `applyDiff(diff)`).
- Assertion `undo-runtime-backed` (§8) — **realized (Phase 1b):** reworded to
  client-owned history (the runtime applies `applyDiff` as an ordinary mutation,
  without owning or navigating a history stack).
- A new realized home (`docs/runtime/mutations/L2` or a dedicated history spec)
  for the log/cursor/sync contract once Phase 2 lands.

## Phasing (plan big, start small)

- **Phase 1 — client-local optimistic cursor (shipped .26, 2026-06-28).** Move
  the stacks to the client, mirror the whole log, persist with the outbox, drop
  the `busyRef` round trip. Ships the **latency + persistence + race** wins
  immediately. Shape the data model for Phase 2 now: **step ids (ULID), not
  session seqs; idempotent id-keyed cursor; durable mirror.** The runtime's
  per-session seq-keyed history is fully retired (Phase 1b: `run_apply_diff`,
  `undo_history`/`redo_history`, `MutationHistory` frame, `DiffStep`,
  `undoOf`/`redoOf` all dropped); the client captures the diff via the
  WASM-exposed `captureMutationDiffJson` (`from_before_after`).
- **Phase 2 — server-authoritative synced log (planned).** Promote the log to
  the per-account store table + `RevLog` view; clients mirror + propose; the server
  arbitrates append/cursor/truncation. Unlocks **cross-device**. Phase 1's id
  keying + idempotent ops make this additive, not a rewrite. Contract:
  [`DESIGN-L2-undo-redo-revlog-contract`](DESIGN-L2-undo-redo-revlog-contract.md).

## Edge cases

- **Offline undo** — fold locally; cursor assignment + applyDiff queue in the
  outbox; converge on reconnect (idempotent assignment tolerates reordering).
- **Message changed externally between action and undo** — delta semantics keep
  the inverse well-defined (removes exactly what was added).
- **Log cap / GC** — the authoritative log is bounded per account (was
  `MAX_HISTORY = 50`); evicted (too-old) steps drop off the undoable tail. Define
  a shared cap so all devices agree on eviction.
- **Global Ctrl+Z across accounts** — the authoritative log is per account; the
  client merges open `RevLog` views by `created_at` for the global shortcut.
- **Account removed / signed out on a device** — that device drops the account's
  mirror; the authoritative log persists for other devices.

## Open questions — resolved (Phase 2 contract)

Resolved in [`DESIGN-L2-undo-redo-revlog-contract`](DESIGN-L2-undo-redo-revlog-contract.md)
(the Phase 2 implementable contract):

- **Global vs per-account ordering** → per-account.
- **Redo-tail durability** → sync the cursor explicitly (`cursor_step_id` +
  `redo_tail` are part of the `RevLog` view).
- **Cap/eviction** → per-account, count-based (`MAX_HISTORY = 50`), oldest-first.
- **Non-message reversibility** → out of scope (messages only).

## Code anchors

- Diff model (reuse): `crates/posthaste-link-core/src/message.rs`
  (`MessageChangeDiff`, `inverse`, `from_before_after`, `apply_message_assertion`).
- History retired (Phase 1b): `crates/posthaste-runtime/src/sessions.rs`
  (`undo_history`/`redo_history`, `pop_*_by_seq`, `push_*`, `history_frame`),
  `crates/posthaste-runtime/src/build.rs` (`run_apply_diff` ~664, `record_diff`),
  `crates/posthaste-runtime-contract/src/lib.rs` (`MutationHistory`, `DiffStep`) —
  all removed.
- Client hook (Phase 1a): `apps/web/src/hooks/useUndoRedo.ts` (round-trip-free;
  `useRuntimeUndoRedo.ts` deleted) + `apps/web/src/runtime/replica/undoHistoryStore.ts`
  (durable `RevStep[]` + cursor).
- Outbox + mirror home: `apps/web/src/runtime/replica/outboxStore.ts`,
  `entityStoreAdapter.ts`.
- Sync-as-view target: `crates/posthaste-runtime/src/views.rs` (`ViewKind`,
  `build_snapshot`, the delta pipeline), `DESIGN-L2-client-link-reactive-store`.
- New store state (Phase 2): `crates/posthaste-store` (per-account `RevLog`).
