---
scope: L2
summary: "From-scratch redesign of undo/redo onto the assertion architecture: enrich each message-change signal with an invertible diff so undo = apply the inverse diff as an ordinary optimistic mutation through the existing outbox + replay(base, unsettled) guard — killing the command-based undo stacks, the inverse-command catalog, the role-move resolution gap, and the undo flicker."
modified: 2026-06-25
reviewed: 2026-06-25
lifecycle: ephemeral
type: DESIGN
depends:
  - path: docs/runtime/mutations/L1
    section: "Mutation pipeline and catalog"
  - path: docs/replication/backend-link/L1
    section: "3. The BackendApi contract"
  - path: docs/replication/client-link/L1
dependents: []
---

# Reversible undo/redo via invertible change-diffs

## The bug this fixes

On dogfood, **every undo flickers**: the view briefly shows the action re-applied
before settling into the undone state. Root cause: undo/redo are the lone holdout
still using a **command-based** model while everything else is assertion-based.

- The renderer "holds no history of its own"; it fires undo as an opaque
  `runMutation({ name: 'mutation.undo', args: {} })`
  (`apps/web/src/hooks/useRuntimeUndoRedo.ts`).
- The client replica derives optimism *from mutation args*
  (`replicaAdapter.ts` `toAssertion`/`translateOptimisticMutation`, ~line 80) —
  it needs `args.messageId` + a known name. With empty args it returns **null**,
  so `runMutation` early-returns and **never puts the undo in the client outbox**.
- So the undo is not in `unsettled`, the `replay(base, unsettled)` clobbering
  guard has nothing to re-fold for it, and a stale optimistic entry (the original
  mutation, still in the outbox) keeps re-folding over the undone base until it
  settles → flicker.

The runtime side *does* fold undo correctly (`run_undo` in
`crates/posthaste-runtime/src/build.rs` ~737 reconstructs the inverse as a full
named request and runs it through the normal `outbox.accept` path), so a pure
bundled view wouldn't flicker — the flicker is the **client replica** starved of
an outbox entry.

## The decision

The link layer is already state/assertion-based: `BaseAssertion` carries a
message's complete mutable state `MessageFoldState { keywords, mailbox_ids }`
(posthaste-link-contract / posthaste-link-core), and both optimism nodes (client
replica, runtime near node) fold these via `project(base, unsettled)`
(`apply_outbox_overlay`, `crates/posthaste-runtime/src/near_node.rs` ~159; the
client replica `projectSnapshot`). **Make undo use that same machinery instead of
a parallel command model.**

Concretely:

1. **Capture an invertible diff for every reversible message mutation.** The
   runtime reads the message's fold state before the provider call (`prev`) and
   again after confirmation (`curr`). The resulting diff is expressed in the
   same add/remove vocabulary:
   `diff = { keywords: { added, removed }, mailboxes: { added, removed } }`.
   The diff is **symmetric/invertible**: `inverse(diff)` swaps added↔removed.
   `prev` is reconstructable as `inverse(diff)` applied to `curr` — so we ship
   **curr + diff, not curr + prev** (most changes are small → small wire cost).
   Role moves are covered for free: the role resolves to concrete mailbox ids, so
   the diff is a concrete `mailboxes.{added,removed}` — no role→mailbox resolution
   needed at undo time.

2. **Undo = apply `inverse(diff)` as an ordinary optimistic mutation.** Redo =
   apply `diff`. Both go through `toAssertion` → client outbox → the replay guard
   → the runtime outbox, identically to any user action. No special-casing, no
   `mutation.undo` opaque command, no defer-entry. The flicker disappears because
   the undo is now a normal `unsettled` entry the guard re-folds.

3. **Shared-session, one owner.** The runtime owns the **seq-ordered** history of
   diffs and broadcasts the current tops on the session stream via
   `mutationHistory` frames (`{ canUndo, canRedo, undoTop?, redoTop? }`); "undo
   the latest by seq" is unambiguous across clients. The *execution* is a plain
   optimistic mutation; the *ordering authority* stays in the runtime. The client
   can undo optimistically once it has received the top diff and sends the
   concrete `inverse(diff)` mutation with `undoOf: seq` (or `redoOf: seq` for
   redo).

4. **Conflict policy: last-writer-wins, no special-casing.** When undo (restore)
   races another client's change, whichever command is applied latest wins. We do
   NOT fail-closed or merge. (`curr` is still available as an optional
   expected-base check for future optimistic-concurrency, but it is not gated on
   for v1.)

## What this retires
- The command-based undo/redo stacks (`undo_stack`/`redo_stack`,
  `HistoryEntry { forward, inverse }`, `MutationCommand`) in
  `crates/posthaste-runtime/src/sessions.rs`.
- `run_undo`/`run_redo` + `mutation.undo`/`mutation.redo` dispatch in `build.rs`.
- The inverse-command catalog (`keyword_history`/`mailbox_history`/the
  non-invertible special cases) — replaced by the uniform invertible diff.
- The `mutationHistory` availability frame may stay (canUndo/canRedo), but is now
  derivable from the seq-ordered diff history.

## Wins beyond fixing undo
- **Role moves solved** (concrete diff, no resolution).
- **Optimistic-concurrency check available** (`curr` = expected base) if we ever
  want stronger-than-LWW later.
- **Deterministic reversible replay** for the offline outbox (local-first effort)
  — queued mutations carry their own inverse.
- One reversibility model; deletes a catalog of inverse logic.

## Cost
- ~2× the *mutable* per-changed-message state on change signals, minimized to
  curr + diff (fold state is tiny). Per changed message, not per view.
- Runtime captures the diff on apply (lateral to today's read-before-write to
  build inverses).
- Non-message mutations (smart mailboxes, settings) are out of scope — same as
  today's message-only undo stack.

## Code anchors (start here)
- Runtime undo/redo + dispatch + outbox: `crates/posthaste-runtime/src/build.rs`
  (`run_undo`/`run_redo` ~737, `dispatch_named_mutation`, `named_message_assertion`,
  `outbox.accept`/`retire` ~680, `*_history` ~706–728).
- Runtime session history + frames: `crates/posthaste-runtime/src/sessions.rs`
  (`undo_stack`/`redo_stack`, `HistoryEntry`, `pop_undo`/`push_redo`/
  `emit_history_frame`, `RuntimeFrame::Notification`, `mail_list_delta`).
- Runtime optimism fold: `crates/posthaste-runtime/src/near_node.rs`
  (`RuntimeBackendOutbox`, `apply_outbox_overlay`, `PendingMessageMutation`).
- Fold state + assertions: posthaste-link-contract (`BaseAssertion`, `BaseUpdate`)
  + posthaste-link-core (`MessageFoldState`, the predictor/convergence).
- Frame/command contract: posthaste-runtime-contract (`RuntimeFrame` variants,
  `MailListDelta`, the mutation request/settlement types).
- Client optimism: `apps/web/src/runtime/replica/replicaAdapter.ts`
  (`toAssertion` ~80, `runMutation` ~183, `projectSnapshot`/the guard ~225–275,
  `settleAll` ~290), `outboxStore.ts`, `handle.ts` (`ReplicaAssertion`),
  posthaste-link-wasm / posthaste-link-replica (`MailListReplica`).
- Client undo hook: `apps/web/src/hooks/useRuntimeUndoRedo.ts`.

## Realization notes

The redesign is implemented across:

- `crates/posthaste-link-core/src/message.rs` — `MessageChangeDiff`,
  `KeywordDelta`, `inverse()`, `from_before_after()`.
- `crates/posthaste-link-core/src/message.rs` +
  `crates/posthaste-runtime-contract/src/lib.rs` — `DiffStep` (seq,
  message id, source id, diff) and `RuntimeFrame::MutationHistory` with
  `undoTop`/`redoTop`.
- `crates/posthaste-runtime/src/{mutation_args,near_node,build,sessions,read}.rs`
  — diff capture, `message.applyDiff` routing, history navigation, and frame
  emission.
- `crates/posthaste-authority-runtime/src/backend.rs` — far-node `applyDiff`
  mapping to backend keyword/mailbox calls.
- `apps/web/src/runtime/replica/{handle,replicaAdapter}.ts` —
  `ReplicaAssertion::applyDiff` + optimistic folding.
- `apps/web/src/hooks/useRuntimeUndoRedo.ts`+
  `apps/web/src/hooks/useEmailActions.ts` — undo/redo now submit
  `message.applyDiff` instead of `mutation.undo`/`mutation.redo`.

### Open questions carried forward

- **Chained-undo latency:** because only the top of each stack is broadcast,
  each undo currently waits for the runtime to confirm and re-broadcast the next
  `undoTop`. A future optimization can ship the full stack to the client or add
  an `undoN` operation so chained undos are local-only.
- **Non-message undo:** settings, account, and smart-mailbox mutations remain out
  of scope, same as the old command-stack model.
