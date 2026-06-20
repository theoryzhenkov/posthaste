---
scope: L2
summary: "Implementation test plan for migrating the bundled app to the shared runtime contract and embedded authority runtime"
modified: 2026-06-20
reviewed: 2026-06-20
lifecycle: ephemeral
type: PLAN
depends:
  - path: docs/runtime/L1
  - path: docs/runtime/L2
  - path: docs/client/L1
  - path: docs/client/L2
  - path: docs/backend/L2
  - path: docs/state/mail/L2
dependents:
  - path: docs/eph/PLAN-L3-api-runtime-wrapper-migration
---

# Bundled app implementation test plan

## 1. Purpose

These tests should drive the migration from “desktop webview talks to embedded loopback API” to “renderer talks to a shared runtime contract through a runtime adapter.”

The bundled desktop app should embed the authority runtime as a crate. It should not require `posthaste serve`, a hidden daemon, or a local replica for ordinary use. Future hosted, multi-device, or offline deployments may embed a different runtime implementation that owns a local replica, outbox, coverage state, and sync with a remote authority. The renderer-facing contract stays the same.

The tests should prove behavior at boundaries. They should not freeze private module names, exact task scheduling, or framework wiring that can change during refactoring.

## 2. Target architecture for the implementing agent

### 2.1 Shared contract, separate implementations

Introduce a runtime contract that is separate from any transport adapter and separate from the authority runtime implementation.

Recommended crate shape:

```text
crates/posthaste-runtime-contract
  RuntimeCore trait or equivalent facade contract
  RuntimeCaller / caller context
  session types
  view descriptors, snapshots, frames, coverage
  named mutation request/receipt/settlement types
  resource request/response types
  RuntimeError / adapter-safe error envelope

crates/posthaste-authority-runtime
  AuthorityRuntimeHandle implements RuntimeCore
  AuthorityRuntimeBuildConfig
  AuthorityRuntimeBuild output
  RuntimeShutdownHandle
  config/secret/store/service/supervisor assembly
  view service, mutation coordinator, event bus/history, resource resolver

future: crates/posthaste-replica-runtime
  ReplicaRuntimeHandle implements RuntimeCore
  local replica, outbox, coverage/gap state, remote-authority sync
```

A temporary single runtime crate is acceptable only if its public contract module is free of authority-only dependencies and can move later without changing renderer or API adapter semantics.

### 2.2 Runtime contract rule

The shared contract must be shaped around runtime operations, not current storage or provider internals.

Good contract inputs and outputs:

- `openRuntimeSession(init)` and `subscribeRuntimeFrames(sessionId, afterSeq)`
- one `RuntimeFrame` sum type for renderer push delivery: view snapshots/replacements/errors/closure, mutation settlement, notifications, and heartbeats
- `openView(sessionId, descriptor, options)`
- `ViewSnapshot { lifecycle, readWatermark, coverage, data, pendingMutations }`
- `runMutation(sessionId, name, args, clientMutationId, context)`
- `MutationReceipt` and settlement frames
- resource requests for bodies, attachments, and source exports

Do not put these in the contract:

- SQLite table names or query fragments
- JMAP/IMAP/SMTP client objects
- local-replica table names
- query invalidation commands
- `/v1/events` renderer cache-invalidation semantics
- Axum extractors/responses, Tauri handles/events, or React component types

### 2.3 Authority runtime implementation

The first implementation is the authority runtime used by the bundled desktop app and by the API server. It owns config/state/cache roots, SQLite, provider gateways, account supervisor, event history, view state, mutation idempotency, resource resolution, and provider secret access.

The authority runtime builder is transport-free. It opens local dependencies and returns an `AuthorityRuntimeHandle` plus shutdown ownership. It does not bind an HTTP listener and does not create Tauri windows.

### 2.4 Adapters

`posthaste-server` becomes an Axum `/v1` adapter over the runtime contract and authority runtime handle. It owns HTTP concerns: extraction, auth/authz, host/origin checks, status codes, error JSON, OpenAPI/AsyncAPI, CORS, tracing, OAuth HTTP redirects, and optional static app serving.

`apps/desktop` becomes a Tauri adapter over the same runtime contract and authority runtime handle. It owns window/session labels, command names, event emission, local capabilities, resource handoff, navigation policy, and temporary loopback bridge containment.

Renderer components call the TypeScript runtime adapter facade. During migration, direct HTTP may remain inside that facade only as a contained bridge.

### 2.5 Current implementation starting points

The existing code still assembles most authority behavior in `crates/posthaste-server/src/lib.rs::start_server` and `AppState`. Existing full-stack API tests build `AppState` in `crates/posthaste-server/tests/support/mod.rs`. The desktop app currently uses desktop bridge commands and loopback connection metadata under `apps/desktop/src` and `apps/web/src/connection`.

The first implementation slice should extract assembly out of `posthaste-server` rather than adding a new `posthaste_server::runtime` module as the long-term owner.

## 3. Test layers

### 3.1 Runtime contract and authority handle tests

Runtime tests exercise the contract and the embedded authority runtime without binding an HTTP listener and without creating Tauri windows.

Suggested locations:

- `crates/posthaste-runtime-contract/src` module tests for contract-only type/trait checks
- `crates/posthaste-authority-runtime/tests/authority_runtime_handle.rs`
- module tests inside the authority runtime for builder, shutdown, views, and mutations

Required behaviors:

- compile the runtime contract without Axum, Tauri, React, provider-client, SQLite-table, or replica-table types
- build an authority runtime handle from temp config/state/cache roots
- initialize config defaults or import bootstrap data
- open the SQLite store and secret store through the authority runtime builder
- start enabled account runtimes using a mock provider gateway
- expose local reads through runtime contract methods
- expose event subscriptions through runtime contract methods
- shut down account runtimes, view subscriptions, queued work, store handles, and provider connections through the runtime shutdown handle
- compile handle methods without Axum extractor/response types, Tauri handles, or frontend component types in their signatures

The first red test should be small: build `AuthorityRuntimeHandle` from empty temp roots and read startup/runtime status. It should fail until the new runtime crate boundary exists. That creates the seam the rest of the migration can use.

### 3.2 API adapter compatibility tests

API adapter tests prove `/v1` still behaves while becoming an adapter over the runtime contract.

Suggested location: existing `crates/posthaste-server/tests/*` harness, extended so it builds the router from an authority runtime handle.

Required behaviors:

- `build_api_router` or its replacement receives API adapter state that wraps `RuntimeCore` or `AuthorityRuntimeHandle`
- existing auth, authz, OpenAPI, AsyncAPI, and full-stack API tests keep passing
- API reads and runtime-handle reads use the same projection constructors for overlapping state
- message command routes call named mutation/runtime command paths or shared mutation helpers, not a duplicate service/store graph
- `/v1/events` consumes the same event history/bus as runtime sessions for API compatibility and integration clients, but renderer behavior moves to `RuntimeFrame::Notification` on the session stream
- host validation still runs before auth exemptions
- bearer tokens in query parameters remain rejected

This layer protects MCP, scripts, tests, debugging, external clients, and temporary loopback clients while the renderer moves away from direct HTTP calls.

### 3.3 Renderer adapter tests

Renderer adapter tests prove React code targets the runtime facade instead of HTTP, provider APIs, or local mail repair.

Suggested locations:

- `apps/web/test/runtimeAdapter.test.ts`
- `apps/web/test/runtimeViewHooks.test.tsx`
- `apps/web/test/runtimeCommands.test.tsx`

Required behaviors:

- bundled bootstrap selects the embedded-runtime adapter
- direct mail HTTP clients are contained inside the adapter module while the loopback bridge exists
- view hooks open serializable descriptors and receive view frames on the single session-scoped `RuntimeFrame` stream
- hooks replace subscriptions when `updateView` returns a different `ViewId`
- hooks pass through `coverage` and `readWatermark` without interpreting freshness locally
- hooks recover from missed frames by resubscribing with `afterSeq` and accepting collapsed full snapshots for active views
- list components request initial, next, previous, and around-anchor windows through the adapter
- list components preserve or reset scroll from runtime anchor status and do not synthesize rows
- command hooks submit catalogued named mutations with stable `ClientMutationId`
- archive/trash/inbox/junk actions submit role intent rather than resolving role mailboxes from cached lists
- undo/retry/conflict UI calls `mutation.undo`, `mutation.retry`, or `mutation.resolveConflict`
- renderer command code does not store mail before-images or a mutation replay log

These tests should use fake runtime adapters rather than a real backend. They verify renderer boundaries.

### 3.4 Runtime view tests

Runtime view tests prove `MailListViewState` is runtime-owned and query windows are backend-settled.

Suggested location: `posthaste-authority-runtime` view-service tests once the service exists; temporary coverage can live near store/query tests only while extraction is in progress.

Required behaviors:

- opening a mail-list view returns rows for the exact `QueryScope`, projection kind, sort, and window request
- row keys remain stable across replacement snapshots for the same represented rows
- continuation cursors are opaque and rejected outside their scope/sort/window context
- around-anchor requests return anchor status: kept, moved, removed, or unknown
- state assertions recompute affected active windows and emit `view.replace`
- dependency analysis may skip unaffected views only when safe
- when dependency data is unknown, the runtime recomputes or marks updating before replacement
- visible conversation/detail views refresh when `ConversationRef`, `bodyToken`, or `attachmentToken` changes

These tests should compare runtime windows against store/query projections rather than checking implementation internals.

### 3.5 Mutation coordinator tests

Mutation tests prove local effects, idempotency, provider reconciliation, and settlement.

Suggested location: `posthaste-authority-runtime` mutation-coordinator tests.

Required behaviors:

- read, flag, and tag mutations normalize keywords in the runtime and emit message assertions
- archive/trash/restore role moves resolve role mailboxes in the runtime
- move-to-mailbox rejects inaccessible or missing mailboxes before local effect
- destroy is irreversible by default and settles through runtime state
- draft save persists local draft/body/attachment state before provider upload when allowed
- send persists the mutation record before provider submission
- retrying the same `ClientMutationId` after a simulated crash does not send twice
- reusing a `ClientMutationId` with different args is rejected
- provider delay leaves mutation queued or pending while local views update when allowed
- provider rejection refreshes/settles affected runtime state according to the mutation contract
- conflict settlement includes runtime-provided resolution options
- conflict choice is submitted as a named mutation referencing the original conflict
- undo uses runtime-stored before-image or inverse; renderer before-images are not needed

Use a mock provider gateway that can delay, fail transiently, reject permanently, and count provider command submissions. The duplicate-send test should assert provider submission count, not just mutation state.

### 3.6 Desktop host and Tauri security tests

Desktop tests prove the packaged renderer has only the intended native capability surface.

Suggested locations:

- Rust tests in `apps/desktop/src/lib.rs` for pure validation helpers
- Tauri command tests where feasible
- lab Playwright smoke tests for process-level behavior
- capability-file checks in a small script or Bun test

Required behaviors:

- production windows load packaged assets; development may load only configured local dev origin
- Tauri command inputs reject invalid descriptors and unknown fields or equivalent strict validation
- generic filesystem, shell, process, network, and environment capabilities are not exposed to renderer windows
- external URL opening accepts only allowed schemes such as `http` and `https`
- untrusted in-app navigation is blocked
- email-body links follow the same external-navigation rule
- focused windows share the same runtime state as the main window
- closing windows releases runtime sessions/views through the adapter

The current main-window lab smoke can remain, but add tests that prove runtime-backed readiness rather than only route-backed surface readiness once the adapter exists.

### 3.7 Storage and secret tests

Storage/security tests prove secrets and mail authority state do not leak into renderer-owned storage or logs.

Suggested locations:

- `posthaste-authority-runtime` storage/security tests
- `crates/posthaste-server/tests/*` for API/loopback perimeter behavior
- `apps/web/test/clientStore.test.ts`
- `apps/desktop` Rust tests for client profile commands

Required behaviors:

- runtime config/state/cache roots are resolved before handle construction and are not renderer-owned paths
- config files contain secret references/redacted status, not provider credentials or tokens
- OAuth refresh writes only the runtime secret store, not config or renderer storage
- renderer local stores reject/preserve absence of provider secrets, bearer tokens, bodies, attachments, event history, DB copies, and mutation idempotency records
- connection-profile JSON contains no tokens
- desktop client profile tokens live in the client keyring service, distinct from runtime provider secrets
- loopback bridge tokens use authorization headers or equivalent non-URL channels
- loopback query-token auth is rejected
- loopback bridge is local-only, host-validated, and origin/referer-checked when present
- bundled mode does not write daemon-style port/token files
- logs do not include Authorization headers, bearer tokens, capability URLs, sensitive query strings, message body content, or attachment bytes

### 3.8 Bundled end-to-end tests

Bundled E2E tests prove the app works as a packaged local authority, not as a web client pointed at a daemon.

Suggested location: `tools/lab/tauri-playwright/`.

Required behaviors:

- app starts with no `posthaste serve` process running
- main window opens only after runtime readiness
- opening a mailbox view uses local runtime projections before provider round trips
- regular mutation local phase updates visible rows before provider completion
- two windows observing the same message or list update from the same runtime mutation
- provider failure settles through runtime state visible to the renderer
- app shutdown leaves accepted queued work recoverable on next launch

These tests can be staged. Start with startup/readiness and one fake runtime view, then add provider delay/failure once the mock provider is available through the bundled runtime.

## 4. Migration order

### 4.1 Establish the runtime crates and first red test

Add `posthaste-runtime-contract` and `posthaste-authority-runtime`, or a temporary single runtime crate with a clearly isolated contract module. Add a test-only authority runtime builder using temp roots and a mock secret store/provider. The first passing test should build `AuthorityRuntimeHandle` without HTTP or Tauri and read runtime status.

### 4.2 Move assembly out of `posthaste-server`

Extract the startup assembly currently in `crates/posthaste-server/src/lib.rs::start_server` into the authority runtime builder. Keep behavior the same: config defaults/bootstrap, database open, service construction, source projection sync, event bus, secret store, supervisor, account startup, root key/auth material when API support is enabled.

### 4.3 Wrap the existing API state

Migrate `AppState` toward API adapter state around the runtime contract. Keep existing API tests passing. Avoid changing endpoint behavior while extracting the handle.

### 4.4 Add the renderer adapter facade

Introduce the TypeScript runtime adapter facade with a fake adapter in tests. Move components/hooks to the facade before changing the transport.

### 4.5 Implement the unified runtime frame stream

Add `RuntimeFrame` in the runtime contract and expose one session-scoped server-to-renderer stream. The first slice may carry only `ViewSnapshot` and `ViewReplace` for the `message.keywords_changed` mail-list path, but the envelope must reserve explicit variants for `MutationSettlement`, `Notification`, and `Heartbeat` so later work does not add another renderer push channel.

Commands stay request/response: `openView`, `closeView`, and future `runMutation` return IDs or receipts, then later state arrives on the session stream. Reconnect uses one `afterSeq` cursor. View catch-up collapses to current snapshots; unsettled mutation state can replay as settlement frames; notifications replay only when their source is durable.

The per-view `/v1/views/{view_id}/stream` transport may remain as a migration bridge. It is not the target renderer path.

### 4.6 Implement view snapshots

Add runtime view service tests for `MailListViewState`, then wire the adapter to consume snapshots from `RuntimeFrame` view variants. Keep full replacement snapshots before adding any patch optimization.

### 4.7 Implement notifications and retire renderer `/v1/events`

Move renderer uses of `/v1/events` to `RuntimeFrame::Notification` on the session stream. Keep an integration/API feed separate from renderer behavior; it has different auth, audience, and compatibility guarantees.

### 4.8 Implement named mutations

Move one mutation family at a time: keyword, role move, destroy, draft, send, settings/account. Each family gets runtime tests and renderer command-hook tests before replacing the old operation runner behavior.

### 4.9 Tighten security boundaries

After the renderer no longer depends on direct mail HTTP, narrow Tauri capabilities and loopback exposure. Add checks that fail if generic capabilities or query-token auth return.

### 4.10 Expand bundled E2E

Add process-level tests once the unit/integration seams are stable. E2E should cover only critical paths that cannot be proven in lower layers.

## 5. Test data and fakes

Use factories instead of shared fixtures:

- `makeRuntimeHarness(overrides)` for temp roots, mock secret store, mock provider, and deterministic clock/IDs
- `makeMessage(overrides)` for message records/projections
- `makeQueryScope(overrides)` for view tests
- `makeRuntimeSnapshot(overrides)` for client hook tests
- `makeMutation(overrides)` for settlement tests

Mock only true system boundaries: provider network, OS keyring, platform paths, and time/randomness. Prefer real stores, real query evaluators, real projection constructors, and real authorization middleware where practical.

## 6. Done criteria

The migration is ready for app-code rollout when these are true:

- the runtime contract is available outside `posthaste-server`
- the authority runtime handle can be built and shut down without HTTP/Tauri
- existing API contract/auth tests pass through the handle-backed router
- bundled desktop embeds the authority runtime crate rather than depending on a hidden daemon
- renderer mail components use the runtime adapter facade in tests
- view snapshot tests cover row identity, cursors, anchors, and replacement frames
- mutation tests cover durable idempotency, local phase, provider failure, conflict, retry, undo, and send duplicate prevention
- storage/security tests prove secrets and mail authority state stay out of renderer storage/logs/URLs
- bundled E2E starts without an external daemon and observes shared runtime state across windows

## 7. Handoff notes

The spec direction is intentional: bundled/local-authority mode does not need a local replica. The embedded authority runtime is the local authority. Future local-replica deployments should implement the same runtime contract with a different runtime implementation that owns replica coverage/outbox/sync state.

Do not implement query invalidation as the runtime contract. Use runtime-authored state assertions, full replacement view snapshots, named mutations, settlement frames, and notification frames on the single session stream.

A single ordered SSE stream can head-of-line-block a notification behind a large view snapshot. Bounded mail-list windows make that acceptable for this migration. If it becomes a problem, change transport behind the same `RuntimeFrame` contract rather than adding another renderer push surface.

Do not expose provider secrets, provider clients, SQLite handles, or runtime secret references to renderer code. Temporary loopback HTTP must remain inside the runtime adapter facade and must use local non-URL capabilities.

## 8. Assertions

| ID | Sev. | Assertion |
| --- | --- | --- |
| runtime-contract-crate-first | MUST | The first implementation slice creates a runtime contract outside `posthaste-server`, or a temporary runtime crate with a contract module free of authority-only dependencies. |
| authority-runtime-handle-test-first | MUST | The first authority implementation test builds the authority runtime handle without HTTP or Tauri. |
| contract-no-transport-types | MUST | Runtime contract types do not use Axum, Tauri, React, provider-client, SQLite-table, or replica-table types. |
| api-adapter-regression | MUST | Existing API/auth/contract tests continue to pass through the handle-backed API adapter. |
| renderer-adapter-tests | MUST | Renderer hooks/components are tested against the runtime adapter facade rather than direct mail HTTP calls. |
| renderer-one-frame-stream | MUST | Renderer push delivery uses one session-scoped `RuntimeFrame` stream; `/v1/events` is not a renderer cache-invalidation path. |
| view-window-tests | MUST | Runtime view tests cover row identity, continuation, anchors, replacement snapshots, and recomputation. |
| mutation-idempotency-tests | MUST | Mutation tests prove accepted `ClientMutationId` dedupe survives retry/crash before provider submission. |
| send-no-duplicate-test | MUST | Send tests assert a reused `ClientMutationId` cannot submit to the provider twice. |
| storage-secret-tests | MUST | Storage/security tests prove secrets and mail authority state do not persist in renderer-owned storage, logs, URLs, or packaged assets. |
| bundled-e2e-no-daemon | MUST | Bundled E2E proves the app starts without a separately running daemon. |
