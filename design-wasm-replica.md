# Design: WASM-ify the TypeScript client replica (#8)

## Problem

Three pieces of client-side replication logic are currently duplicated in TypeScript while an equivalent Rust implementation already exists in `posthaste-link-core` / `posthaste-link-replica`:

1. **Mutation-name → assertion mapping** (`apps/web/src/runtime/replica/replicaAdapter.ts::toAssertion`).
   Mirrors `posthaste-link-contract::message_mutation::MessageMutation::to_assertion`.
2. **Diff inversion** (`apps/web/src/runtime/replica/handle.ts::invertMessageChangeDiff`).
   Mirrors `posthaste-link-core::MessageChangeDiff::inverse`.
3. **Base + delta + pending fold** (`replicaAdapter.ts::applyRuntimeDelta`, `onBaseFrame`, `runMutation`).
   Mirrors `posthaste-link-replica::MailListReplica` plus the runtime's `MailListViewState` delta reconciliation.

The result is a single language-mirror for every message mutation we make locally foldable, plus two copies of the undo/redo invertibility law.

## Goal

Move language-independent behavior into Rust/WASM so the TypeScript adapter mainly orchestrates frames, outbox persistence, and renderer communication — not replication semantics. Keep the existing JSON-string WASM boundary and the `one-replica` invariant.

## Proposed WASM surface

Add these functions to `posthaste-link-wasm`.

### 1. `parse_message_mutation`

```rust
/// Parse a runtime mutation request and, if it is locally foldable, return
/// `{ messageId, assertion }` as JSON. Returns `null` for pass-through
/// mutations (role moves, archive, trash, etc.).
#[wasm_bindgen(js_name = parseMessageMutation)]
pub fn parse_message_mutation(request_json: &str) -> Result<Option<String>, JsError>;
```

- Input: a JSON `MutationRequest` (name + args), matching the runtime downlink shape.
- Output: serialized `MessageAssertion` using the existing tag format
  (`{ kind: "setKeywords", add, remove }`, `{ kind: "replaceMailboxes", mailboxIds }`,
  `{ kind: "destroy" }`, `{ kind: "applyDiff", diff }`).
- Implementation: reuse `posthaste_link_contract::message_mutation::MessageMutation::from_request`
  and `to_assertion`, which is now the single source of truth for near/far/client nodes.

### 2. `invert_message_change_diff`

```rust
/// Swap added↔removed for both keywords and mailboxes.
#[wasm_bindgen(js_name = invertMessageChangeDiff)]
pub fn invert_message_change_diff(diff_json: &str) -> Result<String, JsError>;
```

- Input/output: JSON `MessageChangeDiff` in camelCase.
- Implementation: deserialize via `posthaste_link_core::MessageChangeDiff::inverse`.

### 3. (Optional / Phase 2) `RuntimeMailListReplica`

The current `MailListReplicaHandle` owns only `{ messageId, projection }` rows, so the TS adapter still reconstructs `RuntimeMailListRowState[]` for deltas and re-merges projections. To fully unify the base+delta+pending fold, introduce a new WASM handle backed by the runtime's `MailListViewState` rows.

```rust
#[wasm_bindgen]
pub struct RuntimeMailListReplica {
    engine: MailListReplica,
    rows: Vec<MailListRowState>, // full runtime rows, keyed by row_key
}

impl RuntimeMailListReplica {
    pub fn new() -> Self;

    /// Adopt a served `MailListViewState` rows array as the confirmed base.
    #[wasm_bindgen(js_name = ingestViewJson)]
    pub fn ingest_view_json(&mut self, rows_json: &str) -> Result<(), JsError>;

    /// Apply a `MailListDelta`: merge upserts by row_key and, if present,
    /// reorder/drop rows by `order`. Then rebuild the replica base.
    #[wasm_bindgen(js_name = applyDeltaJson)]
    pub fn apply_delta_json(&mut self, delta_json: &str) -> Result<(), JsError>;

    /// Accept a pending mutation by assertion JSON.
    #[wasm_bindgen(js_name = acceptJson)]
    pub fn accept_json(&mut self, accept_json: &str) -> Result<(), JsError>;

    /// Settle a pending mutation.
    pub fn settle(&mut self, mutation_id: &str, outcome: &str) -> Result<bool, JsError>;

    /// Return the projected rows (full `MailListRowState[]`), with pending
    /// mutations folded in. Optional `mailbox_id` drops rows no longer
    /// belonging to a concrete mailbox.
    #[wasm_bindgen(js_name = projectViewJson)]
    pub fn project_view_json(&self, mailbox_id: Option<String>) -> Result<String, JsError>;
}
```

`ingest_view_json` / `apply_delta_json` keep full row metadata intact so the renderer sees stable `rowKey`, `resourceRef`, `orderKey`, etc.

## Proposed Rust changes

### New `posthaste-link-wasm` module `src/mutation.rs`

Expose `parse_message_mutation` and `invert_message_change_diff` as free wasm-bindgen functions.

Dependencies:

- `posthaste-link-contract` (for `MessageMutation`).
- `posthaste-runtime-contract` (for `MutationRequest`).
- Existing `posthaste-link-core` (for `MessageChangeDiff`).

Verify wasm32 bundle size; `link-contract` pulls in `posthaste-domain` and the runtime contract,
but they are dominated by serde types, which already exist in the client dependency tree indirectly.

### New `posthaste-link-wasm` module `src/view_replica.rs` (Phase 2)

Implement `RuntimeMailListReplica` as a wasm-bindgen wrapper around `MailListReplica` plus the runtime row set.

### Add `apply_delta` to `posthaste-link-replica::MailListReplica`

Even the partial Phase-2 approach can reuse a core helper:

```rust
pub fn apply_delta(&mut self, order: Option<Vec<String>>, upserts: Vec<MailListRow>) {
    // build map, merge upserts, reorder/drop, then replace_base + self.rows = new_rows
}
```

This keeps the row-array math in Rust and makes the WASM surface a thin JSON adapter.

## Proposed TypeScript changes

1. **`apps/web/src/runtime/replica/handle.ts`**
   - Keep `ReplicaHandle`, `ReplicaHandleFactory`, `SettlementVerdict`, `OutboxRecord` logic.
   - Remove `KeywordDelta`, `MessageChangeDiff`, and `invertMessageChangeDiff`; replace by generated/imported WASM types/functions.
   - Keep `ReplicaAssertion` as a TS discriminated union for quick consumption, or — if type generation is enabled — import it.

2. **New `apps/web/src/runtime/replica/wasmUtil.ts`**
   - Typed wrappers for `parseMessageMutation`, `invertMessageChangeDiff`, and generated blob helpers.

3. **`apps/web/src/runtime/replica/replicaAdapter.ts`**
   - Replace `toAssertion` with `parseMessageMutation(request)`.
   - In `runMutation`, use the parsed output both for the outbox record and for `handle.acceptJson`.
   - Phase 2: replace `applyRuntimeDelta` + `applyOptimisticRows` with `RuntimeMailListReplica`.

4. **`apps/web/src/hooks/useRuntimeUndoRedo.ts`**
   - Import `invertMessageChangeDiff` from `wasmUtil` instead of `handle.ts`.

## Type-generation recommendation

Add `ts-rs` to `posthaste-link-core` and `posthaste-link-contract` and generate:

- `MessageChangeDiff`, `KeywordDelta`
- `MessageAssertion`

Check the generated files into `apps/web/src/generated/` under a CI freshness gate.
This directly addresses the review concern that the TS mirror can drift.

If the team wants to keep JSON strings only, start without generation and add a round-trip test in `vitest` that verifies `invertMessageChangeDiff` matches the WASM result.

## Implementation phasing

### Phase 1 — parse + invert (contained, ~2 days)

- Add `parseMessageMutation` and `invertMessageChangeDiff` to `posthaste-link-wasm`.
- Wire them into `replicaAdapter.ts` and `useRuntimeUndoRedo.ts`.
- Add WASM unit tests + one web integration test.
- Consider adding `ts-rs` generation as part of this phase or a fast-follow.

This phase already eliminates the duplicated mutation map and diff inversion, the highest-value, highest-risk duplication.

### Phase 2 — full base+delta fold (larger, ~1 week)

- Add `MailListReplica::apply_delta`.
- Implement `RuntimeMailListReplica` in `posthaste-link-wasm`.
- Rewrite `ReplicaController` to push frames straight into the WASM handle and emit projected rows.
- Delete `applyRuntimeDelta` and `applyOptimisticRows`.
- Add property/convergence tests.

## Risks and caveats

- **Wasm32 bundle size.** `posthaste-link-contract` is heavier than `posthaste-link-core`. We should measure the `.wasm` artifact before/after; if it grows unexpectedly, move the assertion mapping into a smaller crate (e.g. `posthaste-link-core` or a dedicated `posthaste-link-message` crate).
- **Type coupling.** Storing full `MailListRowState` in the WASM handle couples `posthaste-link-wasm` to `posthaste-runtime-contract`. This is acceptable because the wasm crate already exists to bridge the runtime<→client contract, but a lighter alternative is to keep only row metadata in JS and let WASM own projections.
- **`link-contract` not already in wasm build.** We need to confirm it builds for `wasm32-unknown-unknown` and that its transitive dependencies do not introduce I/O or non-wasm crates.
- **Outbox persistence format.** The outbox stores `ReplicaAssertion`. WASM parse returns the same camelCase JSON, so the IndexedDB schema stays unchanged; no migration needed.
- **Undo/redo framing.** `useRuntimeUndoRedo` currently mutates the diff object synchronously. Calling into WASM per undo is fast enough but adds an async boundary if we use the dynamic module import; the wrapper can pre-resolve the factory.

## Open questions

1. Should we implement Phase 1 only, or proceed straight through Phase 2?
2. Do we want generated TS types now, or keep them hand-maintained with a round-trip test?
3. Should `parseMessageMutation` live in `posthaste-link-wasm`, or should `MessageMutation` be moved to a smaller crate to keep wasm bundles lean?
