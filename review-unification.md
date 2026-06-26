# Posthaste — Unification Targets Review

> Review date: 2026-06-26. Scope per task: seven duplication hotspots + test helpers across `crates/` and `apps/web`. The requested `review-context.md` was not present in the repo, so this report is grounded in the task scope and the related audit files (`review-fat-legacy.md`, `review-fragility.md`, `review-tests-docs.md`).

## Executive summary

The codebase already has one strong unification mechanism — the `for_each_link_op!` x-macro in `posthaste-link-contract` keeps the remote link wire, `RemoteBackend`, and `link_router` from drifting. The biggest remaining unification wins are:

1. **Bridge the client-side and authority-side optimistic folding.** The client replica in `apps/web` and the Rust near-node in `posthaste-runtime` independently translate runtime mutation names into the same assertion vocabulary and fold them over served bases. This should live in `posthaste-link-replica` / `posthaste-link-wasm`.
2. **Rename / re-home `AuthorityRuntime*` types in `posthaste-runtime`.** The near-node crate owns `AuthorityRuntimeBuildConfig`, `AuthorityRuntimeHandle`, etc., while `posthaste-authority-runtime` builds the far node. The naming is backwards and confusing.
3. **Collapse duplicate error/code spaces.** `RuntimeErrorCode` and `ApiErrorCode` map to the same domain concepts through hand-written conversion tables.
4. **Extract generic React mutation/view hooks.** `useEmailActions`, the settings editors, and compose all reimplement the same `useMutation({ mutationFn: () => runtimeMutations... })` wrapper.
5. **Share test store/TempDB setup.** `temp_root()` + `DatabaseStore::open(...)` appears in almost every store and server integration module.

Below are specific targets with file paths, line numbers, what to unify, why, and level-of-effort.

---

## 1. `posthaste-runtime` ↔ `posthaste-authority-runtime` mirror logic

### 1.1 Mis-placed `AuthorityRuntime*` types in the near-node crate
- **Files:**
  - `crates/posthaste-runtime/src/build.rs`: `AuthorityRuntimeBuildConfig` (line 52), `AuthorityRuntimeBuildError` (~line 136), `AuthorityRuntimeCore` (line 316), `AuthorityRuntimeHandle` (line 350), `AuthorityRuntimeShutdownError` (~line 1340).
  - `crates/posthaste-authority-runtime/src/build.rs`: imports all of those from `posthaste-runtime` (lines 18-20).
- **What to unify:** Drop the `Authority` prefix from the near-node types (`posthaste-runtime` is the renderer/lean node, not the authority). Rename to `RuntimeBuildConfig`, `RuntimeHandle`, `RuntimeBuildError`, etc.; keep `AuthorityRuntimeBuild` and `BackendNode` semantic names in `posthaste-authority-runtime`.
- **Why:** The names are backwards after the runtime↔authority split and force every consumer to import `AuthorityRuntime*` from `posthaste-runtime`, which is confusing.
- **Effort:** Small (mechanical rename + re-export aliases for backward compat if needed).

### 1.2 `dispatch_named_mutation` (near) and `apply_named_message_mutation` (far) both parse the same mutation args
- **Files:**
  - `crates/posthaste-runtime/src/build.rs`: `dispatch_named_mutation` (line 547), used to capture diffs and route to the backend link.
  - `crates/posthaste-authority-runtime/src/backend.rs`: `apply_named_message_mutation` (line ~701), applies the same named mutations to the far-node backend.
  - `crates/posthaste-runtime/src/mutation_args.rs`: shared arg structs already exist (`MessageSetKeywordsMutationArgs`, `MessageReplaceMailboxesArgs`, etc.), but parsing/dispatch tables are still duplicated in the two build files.
- **What to unify:** Move a single message-command dispatch table into `posthaste-link-contract` or `posthaste-link-core`. The table should take raw `MutationRequest` args and produce either a typed command or a `MessageAssertion`. Both the near-node diff capture and the far-node application can then call the same table.
- **Why:** Adding a new message mutation today requires editing both `runtime/build.rs` and `authority-runtime/backend.rs`, plus sometimes the client replica and `link-contract` paths. A single table prevents skew.
- **Effort:** Medium (need to preserve session-scope/`client_mutation_id` handling and error mapping).

### 1.3 `LocalBackend` manually implements every `BackendApi` delegation
- **Files:**
  - `crates/posthaste-authority-runtime/src/local_backend.rs`: lines 89-444 implement ~40 `BackendApi` methods, each a one-line delegation to `self.backend.$method(...)`.
  - `crates/posthaste-link-contract/src/lib.rs`: `for_each_link_op!` macro already exists for the wire side; `LocalBackend` does not use it.
- **What to unify:** Generate `LocalBackend`'s read/write delegations from the same op table used for `RemoteBackend` and `link_router`. If `Backend` trait shape doesn't line up exactly, introduce a thin `BackendApi` bridge trait or macro.
- **Why:** Every new link op currently needs a matching `Backend` method, a `LocalBackend` delegation, the macro row, a route, and a client method. Removing the manual `LocalBackend` layer cuts the boilerplate in half.
- **Effort:** Medium (requires aligning `Backend` method signatures with the link op table).

---

## 2. Outbox logic duplication

### 2.1 String encoding of `OperationState`/`OperationKind`/`OperationEntityKind` is duplicated in the store
- **Files:**
  - `crates/posthaste-domain/src/model/outbox.rs`: the canonical model enums (`OperationState`, `OperationKind`, `OperationEntityKind`) already derive `Serialize`/`Deserialize` but only in camelCase (lines 32-55, 55-78, 129-140).
  - `crates/posthaste-store/src/outbox.rs`: `parse_operation_state` (line 7), `operation_state_str` (line 23), `parse_operation_kind` (line 32), `operation_kind_str` (line 47), `parse_entity_kind` (line 59), `entity_kind_str` (line 69) all re-map the same variants to the store's wire strings (`"pending"`, `"setKeywords"`, etc.).
- **What to unify:** Implement `Display` + `FromStr` (or use `strum`) on the domain enums with the exact database string representation, and delete the store-local parsing functions. The store can then call `state.to_string()` and `OperationState::from_str(value)?`.
- **Why:** Two translation layers for the same three enums. A new state/kind requires touching both model and store.
- **Effort:** Small.

### 2.2 Outbox operation test builders are repeated
- **Files:**
  - `crates/posthaste-store/src/tests/outbox.rs`: `fn operation(...)` builder (line 10) is local to this file.
  - Similar `Operation { ... }` literals appear in `crates/posthaste-domain/src/service/tests/outbox.rs`, `message_mutation_retries.rs`, `message_mutation_cursors.rs`.
- **What to unify:** Move a canonical `operation()` and `draft_operation()` builder set into a shared test fixture crate (see §8).
- **Why:** Minor hygiene; avoids subtle differences in timestamps/attempts across suites.
- **Effort:** Small.

---

## 3. Replication / coherent-link duplication: client-side vs authority-side

### 3.1 Mutation-name → assertion mapping is duplicated in Rust near-node and TS client replica
- **Files:**
  - `crates/posthaste-runtime/src/near_node.rs`: `named_message_assertion` (lines 91-156) maps `message.setKeywords`, `message.setReadState`, `message.setFlaggedState`, `message.setUserTags`, `message.moveToMailbox`, `message.replaceMailboxes`, `message.destroy`, `message.applyDiff` into `posthaste_link_core::MessageAssertion`.
  - `apps/web/src/runtime/replica/replicaAdapter.ts`: `toAssertion` (lines 98-137) maps the exact same runtime mutation names into the TS `ReplicaAssertion` type.
- **What to unify:** Expose the assertion vocabulary and the mapping from a runtime mutation request through `posthaste-link-replica` / `posthaste-link-wasm` (which already contains the Rust `MessageAssertion`). The TS adapter should receive the assertion from WASM or at least import a generated table, rather than maintaining a second copy.
- **Why:** Adding, removing, or renaming a message mutation currently requires editing the Rust near node, the WASM handle interface, and the TypeScript replica adapter. A single source for "which runtime mutations are locally foldable" is the contract `one-replica` promise.
- **Effort:** Large (requires exposing the mapping across the WASM boundary and possibly code-generating the TS enum).

### 3.2 `MessageChangeDiff` / `KeywordDelta` duplicated in TypeScript
- **Files:**
  - `crates/posthaste-link-core/src/message.rs`: `KeywordDelta` (line 34) and `MessageChangeDiff` (line 58) with `inverse()` (lines 40-43, 67-73).
  - `apps/web/src/runtime/replica/handle.ts`: `KeywordDelta`, `MessageChangeDiff` interfaces and `invertMessageChangeDiff` (lines 23-39).
- **What to unify:** `posthaste-link-wasm` already exposes the replica handle; extend the WASM interface to expose diff types/helpers so the TS code can call `handle.invertDiff(diff)` or import a generatedshape instead of re-declaring `MessageChangeDiff`.
- **Why:** The invertibility law (undo/redo correctness) is implemented twice in different languages.
- **Effort:** Medium (WASM bindings + generated TS types).

### 3.3 Delta reconciliation over served bases is duplicated
- **Files:**
  - `crates/posthaste-runtime/src/near_node.rs`: `apply_outbox_overlay` (lines 185-231) folds pending mutations over served rows using `MailListReplica`.
  - `apps/web/src/runtime/replica/replicaAdapter.ts`: `applyRuntimeDelta` (lines 70-84), `onBaseFrame` (lines 219-293), and `runMutation` (lines 171-204) perform the same base+delta+pending fold in TypeScript.
- **What to unify:** Both should delegate to `posthaste-link-replica::MailListReplica` (already used by the Rust near node). The client replica should do its folding in WASM and only emit view frames, rather than reimplementing delta application and pending tracking in TS.
- **Why:** Two implementations of the same core replication invariant makes divergence likely.
- **Effort:** Large (reshapes the client replica architecture).

---

## 4. Web runtime adapter duplication

### 4.1 Repeated `useMutation({ mutationFn: () => runtimeMutations... })` wrappers
- **Files:**
  - `apps/web/src/components/settings-panel/useAccountCommandMutation.ts` (lines 15-91)
  - `apps/web/src/components/settings-panel/AccountEditor.tsx` (lines 82, 109)
  - `apps/web/src/components/settings-panel/AccountAppearanceFields.tsx` (line 39)
  - `apps/web/src/components/settings-panel/SourceMailboxEditor.tsx` (line 56)
  - `apps/web/src/components/settings-panel/SmartMailboxEditor.tsx` (line 87)
  - `apps/web/src/components/settings-panel/automation-actions/AutomationRuleEditor.tsx` (line 51) and `linkedAutomationRules.tsx` (line 53)
  - `apps/web/src/components/compose-overlay/useComposeSubmission.ts` (lines 30-45)
  - `apps/web/src/components/compose-overlay/useComposeAutosave.ts` (implicit via `runtimeMutations.messages.saveDraft`)
  - `apps/web/src/components/settings-panel/OutboxSection.tsx`
- **What to unify:** Introduce `apps/web/src/hooks/useRuntimeMutation.ts` — a generic factory that takes a `runtimeMutations` method and optional invalidation keys, returning a `useMutation` with consistent pending/error/toast behavior. Use it for all settings/compose/account CRUD mutations.
- **Why:** Dozens of call sites each repeat `useMutation`, `runtimeMutations` import, `queryClient`, and cache invalidation. A factory collapses those to one-liners and makes it easier to add uniform error reporting / retry logic later.
- **Effort:** Small to Medium.

### 4.2 `useEmailActions` embeds a generic action dispatcher
- **Files:**
  - `apps/web/src/hooks/useEmailActions.ts`: the `dispatch()` helper (lines 110-149), `setPending` / `isPending` plumbing, and toast wiring are generic and reimplemented in other action hooks.
- **What to unify:** Extract a `useRuntimeDispatch` hook that owns pending count, toast/undo wiring, and error message state. `useEmailActions` then only maps mail actions to mutation calls.
- **Why:** `useEmailActions` is ~380 lines and mixes generic dispatch plumbing with mail-specific keyword logic.
- **Effort:** Small.

### 4.3 `useRuntimeObjectView` and `useRuntimeMailListView` share subscription handling
- **Files:**
  - `apps/web/src/runtime/useRuntimeObjectView.ts`
  - `apps/web/src/components/message-list/useRuntimeMailListView.ts` (handles `viewSnapshot`, `viewReplace`, `viewDelta`, `mutationSettlement`, `mutationHistory`)
  - `apps/web/src/hooks/useAccountsView.ts` is a thin `useRuntimeObjectView` wrapper.
- **What to unify:** Pull a shared `useRuntimeSubscription` hook from `useRuntimeMailListView` that handles stream lifecycle, seq tracking, and frame routing. Both object-view and mail-list-view hooks become consumers.
- **Why:** Frame routing and cleanup logic is duplicated; `useRuntimeObjectView` already has a TODO-like silence for several frame types.
- **Effort:** Small.

### 4.4 `runtime/adapter.ts` `unsupportedRuntimeAdapter` duplicates the `RuntimeAdapter` interface surface
- **Files:**
  - `apps/web/src/runtime/adapter.ts` lines 23-176 list every method in `RuntimeAdapter`.
  - `apps/web/src/runtime/types.ts` lines 302+ define `RuntimeAdapter`.
- **What to unify:** Use a TypeScript `Proxy` to produce a rejecting adapter from the interface keys, or code-generate the stub from the same source that produces `types.ts`.
- **Why:** Adding a method to `RuntimeAdapter` requires touching both the interface and the unsupported stub.
- **Effort:** Tiny if Proxy is acceptable; the typing can be preserved.

---

## 5. Error types that are nearly identical across crates

### 5.1 `RuntimeErrorCode` and `ApiErrorCode` overlap heavily
- **Files:**
  - `crates/posthaste-runtime-contract/src/lib.rs`: `RuntimeErrorCode` (line 748).
  - `crates/posthaste-api/src/api/errors.rs`: `ApiErrorCode` (line 13).
  - `crates/posthaste-api/src/api/errors.rs`: `runtime_error_status_code` mapping table (line ~240) and `From<ServiceErrorKind>` (line 65).
- **What to unify:** Collapse to one shared code enum. The cleanest path is to make `ApiErrorCode` a superset/wrapper of `RuntimeErrorCode` (or vice versa). The domain `ServiceErrorKind` can map directly into the shared code space, eliminating two manual mapping tables.
- **Why:** 15+ codes are semantically identical (`NotFound`, `Conflict`, `NetworkError`, `StateMismatch`, `GatewayRejected`, `SecretUnavailable`, `StorageFailure`, `Config*`, etc.). Two hand-written tables are a recipe for drift.
- **Effort:** Medium — touches the public `/v1` wire contract and requires a migration plan for existing clients.

### 5.2 `RuntimeAdapterError` / `MutationSettlement` serialization duplicated in runtime-contract and API wrappers
- **Files:**
  - `crates/posthaste-runtime-contract/src/lib.rs`: `RuntimeAdapterError` (line 735), `RuntimeMutationSettlement` (line 713).
  - `apps/web/src/runtime/types.ts`: mirror interfaces (`RuntimeAdapterError`, `RuntimeMutationSettlementState`).
- **What to unify:** Generate the TS runtime types from the Rust `utoipa` schemas or `openapi.json` / `asyncapi.json`, rather than hand-maintaining the mirror.
- **Why:** Type drift between the Rust contract and the renderer is possible and hard to catch.
- **Effort:** Medium (CI/codegen pipeline).

---

## 6. Serialization / schema generation duplicated between `api`, `runtime-contract`, and `domain`

### 6.1 API request wrappers duplicate runtime-contract mutation structs
- **Files:**
  - `crates/posthaste-api/src/api/accounts/types.rs`: `CreateAccountRequest`/`PatchAccountRequest` (lines 91/111) with `From<...> for CreateAccountMutation/PatchAccountMutation`.
  - `crates/posthaste-api/src/api/settings.rs`: `PatchSettingsRequest` (line 8) for `PatchAppSettingsMutation`.
  - `crates/posthaste-api/src/api/smart_mailboxes/crud.rs`: `CreateSmartMailboxRequest`/`PatchSmartMailboxRequest`.
  - `crates/posthaste-runtime-contract/src/lib.rs`: `CreateAccountMutation` (line 205), `PatchAccountMutation` (line 222), `PatchAppSettingsMutation` (line 235), `CreateSmartMailboxMutation` (line 259), `PatchSmartMailboxMutation` (line 267).
- **What to unify:** Make API handlers accept the runtime-contract mutation types directly, using `#[garde]`, `#[validate]`, or newtype wrappers only when the API needs extra validation. Remove the duplicate schema listings in `openapi.rs` for the wrappers.
- **Why:** Two struct definitions, two `ToSchema` derivations, and `From` impls for every account/settings/smart-mailbox mutation. The API wire is mostly identical to the runtime wire.
- **Effort:** Medium (API validation differences must be preserved).

### 6.2 `asyncapi.json` is hand-maintained while `openapi.json` is generated
- **Files:**
  - `asyncapi.json`
  - `crates/posthaste-server/tests/asyncapi_contract.rs` (only checks the `EventTopic` enum, not payloads)
  - `crimes plus 211openapi.rs` generates OpenAPI from `utoipa`.
- **What to unify:** Derive `utoipa::ToSchema` for event payload structs and generate `asyncapi.json` the same way `openapi.json` is generated. Add a full-document round-trip test like `openapi_contract.rs`.
- **Why:** Payload drift ships silently; see `review-tests-docs.md` §1.6.
- **Effort:** Medium.

---

## 7. Logging/observability patterns not using `posthaste-observability`

### 7.1 `posthaste-runtime` uses raw `tracing` macros
- **Files:**
  - `crates/posthaste-runtime/src/sessions.rs`: lines 18 (`use tracing::{debug, warn}`), 126 (`debug!(...)`), 190/202 (`warn!(...)`), 211 (`debug!(...)`).
- **What to unify:** Convert to `ph_debug!` / `ph_warn!` and add domain-specific events to `crates/posthaste-observability/src/events.rs`.
- **Why:** Every other production crate uses `ph_*!` macros with typed `LogEvent` constants; `posthaste-runtime` is the exception. This makes log aggregation and event index inconsistent.
- **Effort:** Tiny.

---

## 8. Similar test helpers/fixtures across crates

### 8.1 `temp_root()` + `DatabaseStore::open(...)` boilerplate duplicated everywhere
- **Files:**
  - `crates/posthaste-store/src/tests.rs`: `fn temp_root()` (line ~17) and numerous `DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))` in `cache/tests/*.rs`, `tests/*.rs`.
  - `crates/posthaste-server/tests/support/mod.rs`: `fn temp_root()` (line ~54) + `DatabaseStore::open(...)` (line ~98).
  - Same pattern in `crates/posthaste-server/tests/auth_middleware/support.rs`, `automation_preview.rs`, `automation_rules/harness.rs`, `api_boundary_contracts/support.rs`, `settings_patch/support.rs`, `capability_scoping/support.rs`, `backend_link_split.rs`.
- **What to unify:** Create a `crates/posthaste-test-support` crate with helpers like:
  ```rust
  pub fn temp_dir(prefix: &str) -> PathBuf;
  pub fn test_store() -> (PathBuf, Arc<DatabaseStore>);
  pub async fn test_authority_runtime() -> AuthorityRuntimeBuild;
  ```
  Use it from `posthaste-store` and `posthaste-server` integration tests.
- **Why:** The setup is error-prone and copied ~40 times. Centralizing it also makes teardown/cleanup uniform.
- **Effort:** Small.

### 8.2 Test fixtures for `MessageRecord`, `Operation`, and `SmartMailbox` are crate-local
- **Files:**
  - `crates/posthaste-domain/src/service/tests/fixtures.rs`: `sample_message_record`, `sample_smart_mailbox`, `sample_source`, etc.
  - `crates/posthaste-store/src/tests.rs`: `sample_message(...)`.
  - `crates/posthaste-store/src/tests/outbox.rs`: `operation(...)` builder.
- **What to unify:** Move canonical sample builders into `posthaste-test-support` (or a `test-fixtures` feature of `posthaste-domain`) so store/domain/engine tests share the same sample data.
- **Why:** Divergent sample timestamps/IDs cause brittle tests. A shared fixture set makes changes to model defaults safer.
- **Effort:** Small.

---

## Prioritization sketch

| Priority | Target | Files | Effort | Payoff |
|---|---|---|---|---|
| 1 | Rename `AuthorityRuntime*` in `posthaste-runtime` | §1.1 | S | High clarity |
| 2 | Unified `RuntimeErrorCode`/`ApiErrorCode` | §5.1 | M | Removes drift-prone mapping |
| 3 | Generic `useRuntimeMutation` + `useRuntimeDispatch` hooks | §4.1, §4.2 | S-M | Cuts ~300 LOC of boilerplate |
| 4 | Shared test store/temp_root crate | §8.1, §8.2 | S | Hygiene + cleanup |
| 5 | Single link op table for `LocalBackend` | §1.3 | M | Cuts far-node boilerplate |
| 6 | WASM-ify client replica fold/diff | §3.1, §3.2, §3.3 | L | One replica implementation |
| 7 | Generate `asyncapi.json` from schemas | §6.2 | M | Catches contract drift |
| 8 | Runtime session logs → `ph_*!` macros | §7.1 | XS | Consistency |

---

## Correct / already-unified

- `posthaste-link-contract` `for_each_link_op!` macro (`crates/posthaste-link-contract/src/lib.rs:999`) already keeps `RemoteBackend`, `link_router`, and the shared request structs in sync. **This is the pattern to emulate for the remaining targets.**
- `posthaste-link-core` and `posthaste-link-replica` already provide a single Rust implementation of the message fold state and mail-list replica; the duplication is mostly in the TypeScript client not using them.
- `posthaste-runtime/src/mutation_args.rs` already shares argument parsing structs between the runtime near node and the far-node `Backend`, which is the right direction.
