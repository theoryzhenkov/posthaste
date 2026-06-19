---
scope: L3
summary: "Temporary API runtime-wrapper migration controls for moving /v1 from AppState-owned services to the authority runtime handle"
modified: 2026-06-16
reviewed: 2026-06-19
lifecycle: ephemeral
type: PLAN
depends:
  - path: docs/eph/PLAN-L2-bundled-app-test-plan
  - path: docs/runtime/L2
  - path: docs/backend/L2
---

# API runtime-wrapper migration plan

## 1. Purpose

This file records the approved temporary wrapper used while `/v1` migrates from direct `AppState` service/store ownership to the shared authority runtime handle.

The industry pattern is branch by abstraction / strangler-fig migration: introduce the stable runtime seam, route old behavior through a wrapper that preserves behavior, move one behavior family at a time behind the seam, then delete the wrapper.

## 2. Approved temporary exception

`posthaste-server::AppState` contains the `AuthorityRuntimeHandle` target runtime boundary plus HTTP-adapter-owned state. Legacy direct fields such as `MailService`, `MailStore`, `SecretStore`, `AccountSupervisor`, and `event_sender` must not be exposed through route state.

`AuthorityRuntimeBuild::api_bridge` may temporarily expose those handles for compatibility test harnesses and migration constructors while direct seeding paths are retired. Runtime-core method implementations must use explicit runtime-owned dependencies rather than calling through `api_bridge`.

This exception exists only to avoid changing endpoint behavior while the API adapter is extracted.

## 3. Allowed call sites

During the migration, legacy direct access is allowed only in these places:

1. existing `/v1` route handlers that have not yet been moved to runtime-handle methods;
2. auth, authz, OAuth, OpenAPI/AsyncAPI, CORS, host/origin, static-asset, and tracing code that owns HTTP concerns;
3. integration-test harnesses that seed store state or exercise existing endpoint behavior; and
4. the temporary API bridge constructors used to build migration handles in tests.

New mail-state behavior must be added behind `RuntimeCore` or `AuthorityRuntimeHandle` first, then adapted to HTTP. Do not add new independent service/store graphs to route modules.

## 4. Fitness functions

The migration should accumulate tests or checks that make the wrapper hard to forget:

- API router construction has a runtime handle available in `AppState` without legacy service/store/supervisor fields.
- Server startup uses the authority runtime builder for config/store/service/event assembly instead of building that graph directly in `posthaste-server`.
- Existing API/auth/contract tests keep passing through the wrapper.
- When a runtime read/mutation/view method exists, new handler tests assert the handler calls the runtime path or shared helper below the handle.
- Later, a dependency or grep-style check rejects new direct `MailService` or `DatabaseStore` construction in route modules.

## 5. Removal criteria

Delete the wrapper and this ephemeral plan when all are true:

1. `AppState` does not expose `service`, `store`, `secret_store`, `supervisor`, or `event_sender` to route handlers for mail behavior.
2. `/v1` reads use runtime read methods or shared projection helpers owned below the handle.
3. `/v1/events` consumes runtime event history/bus through the handle.
4. message command routes use named mutation/runtime command paths or shared mutation helpers below the handle.
5. account lifecycle and OAuth-specific HTTP routes keep HTTP concerns in `posthaste-server` while delegating runtime behavior to the handle.
6. API tests build router state around the runtime handle; harness-owned service/store/supervisor handles are isolated from route state when needed for fixture seeding.
7. a guard check prevents reintroducing direct route-module service/store construction.

## 6. Migration tag

Temporary code must use this searchable tag:

```text
MIGRATION(api-runtime-wrapper)
```

Each tag should link to this file or to an assertion below.

## 7. Assertions

| ID | Sev. | Assertion |
| --- | --- | --- |
| appstate-has-runtime-handle | MUST | `posthaste-server::AppState` carries an `AuthorityRuntimeHandle` during the wrapper migration. |
| legacy-fields-temporary | MUST | Direct service/store/supervisor handles are temporary migration-constructor or test-harness internals, not `AppState` route fields or runtime-core method dependencies. |
| server-startup-authority-builder | MUST | Server startup uses `posthaste-authority-runtime` to assemble config, store, service, secret store, and event channel before API adapter state is built. |
| no-new-route-service-graphs | MUST | New mail-state route behavior is not implemented by constructing a separate `MailService`, `DatabaseStore`, provider gateway, or supervisor graph in route modules. |
| wrapper-fitness-tests | MUST | Tests verify router/API state has a runtime handle and existing API/auth behavior still passes through the wrapper. |
| wrapper-removal-criteria | MUST | The wrapper cannot be considered complete until the removal criteria in this file are satisfied. |
| migration-tag-required | SHOULD | Temporary wrapper code uses the `MIGRATION(api-runtime-wrapper)` tag so it remains searchable. |
