# Backend architecture context (implementation-derived)

Scope: Rust/backend architecture for SPECial revision. No files edited besides this context note.

## Crate map and dependency direction

Workspace crates (`Cargo.toml`, crate manifests):

- `posthaste-domain`: core model, provider policy, port traits, service orchestration. Depends only on serde/async-trait/futures/time/observability, with optional `utoipa` for API schema derivation. Public surface is re-exported from `crates/posthaste-domain/src/lib.rs`.
- `posthaste-config`: TOML-backed config repository (`crates/posthaste-config/src/lib.rs`, `repository.rs`). Implements the domain `ConfigRepository` boundary.
- `posthaste-store`: SQLite/read-model/event/cache store (`crates/posthaste-store/src/lib.rs`, `store.rs`, `db.rs`). Implements `MailStore` and all narrower store traits from domain.
- `posthaste-engine`: JMAP adapter, live gateway, sync, push transports, compose helpers (`crates/posthaste-engine/src/lib.rs`). Implements `MailGateway` for `LiveJmapGateway` and includes `MockJmapGateway`.
- `posthaste-imap`: IMAP/SMTP adapter boundary (`crates/posthaste-imap/src/lib.rs`). Implements `MailGateway` for `LiveImapSmtpGateway` and maps IMAP/SMTP state into common domain records.
- `posthaste-server`: Axum API, auth/authz, OAuth, secret store, supervisor runtime, OpenAPI/AsyncAPI serving (`crates/posthaste-server/src/lib.rs`). Depends on all backend adapter crates.
- `posthaste-observability`: tracing macros/event constants.
- `posthaste-lab`: separate lab tooling crate; not in primary mail runtime path.

High-level layering from implementation:

```text
API/server/supervisor
  -> domain MailService + ports
  -> adapter implementations (JMAP engine, IMAP/SMTP, SQLite store, TOML config, secret store)
  -> external provider protocols / filesystem / keyring / SQLite
```

The important architectural boundary is domain ports, not protocol-specific abstractions in the UI.

## Domain: model, ports, service boundary

Relevant files:

- `crates/posthaste-domain/src/ports.rs:56` defines `MailGateway`, the provider-facing domain port. It includes sync, lazy body/blob fetch, message/mailbox mutations, identity/reply context, send, and push transport discovery.
- `crates/posthaste-domain/src/ports.rs:622` defines aggregate `MailStore`, composed from narrower read/write traits: mailbox/message/tag/conversation/detail/smart-mailbox reads, sync state, IMAP state/location stores, sync writes, cache, command persistence, event log, source projection/data, sender cache, automation backfills.
- `crates/posthaste-domain/src/ports.rs:672` defines `SecretStore` (resolve/save/update/delete `SecretRef`).
- `crates/posthaste-domain/src/service.rs:33` defines `MailService`, which owns no live protocol connection; it composes `ConfigRepository` plus store trait objects.
- `crates/posthaste-domain/src/model/mod.rs:257`, `:423`, `:587` define `AccountDriver` (`Jmap`, `ImapSmtp`, `Mock`), `AccountTransportSettings`, and `AccountSettings`.
- `crates/posthaste-domain/src/provider.rs:20`, `:71` define `ProviderKind` and `ProviderProfile`, separating vendor/family policy from runtime protocol driver.

Data/command flow through `MailService`:

- Config CRUD delegates to `ConfigRepository`, then syncs source projections/deletes source data in the store (`service.rs:70-146` read in context).
- Read methods compose config + store projections; UI/API reads never go directly to providers.
- Sync: `MailService::sync_account_with_mode` at `crates/posthaste-domain/src/service.rs:565` loads cursors, optionally drops message cursor for full metadata, calls `gateway.sync`, applies the returned `SyncBatch` through `SyncWriteStore`, records cache candidates, applies automation rules, appends `sync.completed`, and returns domain events for supervisor publication.
- Lazy body/blob: message detail can fetch body through gateway and persist it via `apply_message_body`; blobs are streamed through gateway (`service.rs:520-540`).
- Mutations: `crates/posthaste-domain/src/service/mutation.rs:22` sends provider mutation with optimistic state (`sync_cursor` message state when available), handles `StateMismatch` by refreshing, then persists local command result through `MessageCommandStore`. `add_to_mailbox`/`remove_from_mailbox` are local mailbox-list transforms that call `replace_mailboxes`.

Spec-relevant invariant from code: `MailService` requires callers to supply a `&dyn MailGateway` for protocol operations; the supervisor owns live gateway lifecycle and exposes gateway lookup to API handlers.

## Provider boundaries

### JMAP (`posthaste-engine`)

- `crates/posthaste-engine/src/lib.rs` says this crate owns gateway implementations, sync loop, push transports, compose helpers.
- `crates/posthaste-engine/src/live.rs:65` defines `LiveJmapGateway`, holding authenticated `jmap_client::Client` and optional shared WebSocket connection.
- `live.rs:186` implements `MailGateway` for JMAP. Sync delegates to `live_sync::sync_account`; body/blob/identity/reply/send/mutations delegate to live modules. Interactive methods can route over WebSocket if connected, otherwise HTTP (`live.rs:116-139`).
- `connect_jmap_client` performs session discovery/auth and logs session/push capability (`live.rs:19-54`).
- Push transports are WS preferred when server advertises websocket push, then SSE (exported from `lib.rs`; negotiated by supervisor).

### IMAP/SMTP (`posthaste-imap`)

- `crates/posthaste-imap/src/lib.rs` explicitly states this crate owns protocol-facing IMAP behavior while mapping server state into domain records.
- `crates/posthaste-imap/src/gateway.rs:42` defines `LiveImapSmtpGateway` with IMAP config, SMTP config, discovery result, and optional store reference.
- `gateway.rs:800` implements `MailGateway` for IMAP/SMTP:
  - `sync` performs capability refresh/discovery, mailbox planning, mailbox fetches, then returns a `SyncBatch` (`gateway.rs:801-912`).
  - lazy body uses stored `ImapMessageLocation` + mailbox name (`gateway.rs:914-924`).
  - attachment blob downloads fetch raw MIME by location and extract attachment bytes (`gateway.rs:926-938`).
  - keyword/mailbox/delete mutations use IMAP commands and return `MutationOutcome { cursor: None }` because sync cursors are mailbox-scoped IMAP state (`gateway.rs:940-1080`).
  - mailbox role changes are local `mailbox_role_override` writes for IMAP (`gateway.rs:1082-1094`).
  - identity/send use SMTP settings; send may append a Sent copy according to provider policy (`gateway.rs:1096-1144`).
  - `push_transports` returns empty (`gateway.rs:1146`); IMAP IDLE is wired separately by the supervisor as a push-hint stream.
- Provider policy in domain (`provider.rs`) drives Gmail/generic behavior: identity source, thread source, label source, required full sync, remote observation policy, mailbox role aliases, SMTP Sent-copy policy.

Implementation nuance/spec gap: IMAP IDLE support is not represented by `MailGateway::push_transports`; it is supervisor-owned (`imap_idle_event_stream`) and policy-driven. Specs should avoid saying all provider push is obtained from `MailGateway::push_transports`.

## Store boundary and data model

- `crates/posthaste-store/src/store.rs:9` defines `DatabaseStore`: SQLite `db_path`, `data_root`, serialized write connection (`Mutex<Connection>`), read connections opened per read. Raw MIME is content-addressed under `data_root/accounts/{account_id}/messages/{sha_prefix}/{sha}.eml`.
- `crates/posthaste-store/src/db.rs:9` initializes schema. Key tables in implementation include: `mailbox`, `mailbox_role_override`, `message`, `conversation`, `conversation_message`, `message_mailbox`, `message_keyword`, `message_body`, `message_attachment`, `thread_view`, `sync_cursor`, `imap_mailbox_sync_state`, `imap_message_location`, `event_log`, `source_projection`, `automation_backfill_job`, `sender_address_cache`, `cache_object`, `cache_message_signal`, `cache_rescore_queue`.
- Schema uses `(account_id, ...)` composite keys for account-scoped rows. `conversation` itself is global by `id` with `latest_source_id` and `conversation_message` backrefs.
- `crates/posthaste-store/src/commands.rs:33` implements `SyncWriteStore` for `DatabaseStore`; `apply_sync_batch` stages raw bodies to disk before opening the write transaction, then calls transaction logic.
- `crates/posthaste-store/src/mutations.rs:49` is core sync write path: snapshot deletes for `replace_all_mailboxes`/`replace_all_messages`, explicit deleted mailbox/message IDs, IMAP location deletes/upserts, mailbox/message upserts, projection refresh, cursor persistence, and domain event insertion.
- `apply_message_record_tx` assigns/updates conversation projection, message row, mailbox and keyword junctions, body cache row, attachments, and emits diff events (`mutations.rs:288+`).

Spec gap: `docs/L1-sync.md` table list omits `message_attachment` even though schema creates it; update specs if documenting full runtime schema.

## Config and secrets

- `crates/posthaste-config/src/lib.rs` states TOML-backed configuration persistence for accounts and smart mailboxes.
- `crates/posthaste-config/src/repository.rs:19` defines `TomlConfigRepository`, storing config root plus in-memory `ConfigSnapshot` behind `RwLock`.
- `repository.rs:31-70` opens/creates `app.toml` support dirs, initializes defaults, and loads snapshot.
- `repository.rs:88` implements `ConfigRepository`; reads use cached snapshot, writes use atomic TOML writes then update snapshot, reload diffs sources.
- Secrets are not in TOML; API/supervisor uses `SecretStore`. Server implementation is `SystemSecretStore` (not deeply read here), while tests use in-memory or stub stores.
- OAuth2 accounts store encoded token sets in the secret store; `supervisor.rs:1265-1291` decodes/refreshes and writes updated token sets back through `SecretStore::update`.

Spec gap: code references `docs/eph/DESIGN-L1-trust-model` and `docs/eph/DESIGN-L1-capability-tokens` from comments, but `docs/eph` was not present in this workspace. If SPECial is being revised, either restore/convert those specs or remove stale `@spec` references.

## Server/API/runtime boundaries

### Axum API

- `crates/posthaste-server/src/lib.rs:49` defines `AppState`: `MailService`, `MailStore`, `SecretStore`, `AccountSupervisor`, broadcast sender, asset root, OAuth flow store, auth token/root key, auth/CORS/host settings.
- `lib.rs:252` builds the `/v1` router. Route set includes health, settings/automation preview, account CRUD/OAuth/logo, typed `/read`, smart mailboxes, conversations, source mailboxes/messages, global search, sender/identity/reply/send, message commands, sync/config reload, events, OpenAPI/AsyncAPI.
- `crates/posthaste-server/src/api.rs:95` defines typed read-call request; `api.rs:111` supports `Account/list`, `Mailbox/list`, `SmartMailbox/list`, `Tag/list`.
- Handler starts: health `api.rs:870`, read `:893`, account CRUD `:1072+`, mailboxes/messages/conversations `:2007+`, compose `:2458+`, sync/events `:2776`/`:2819`, auth token `:3075`.
- API handlers map `ServiceError` to structured `ApiError` and use camelCase serde types (per docs and types).

### Supervisor/runtime

- `crates/posthaste-server/src/supervisor.rs:45` defines `AccountSupervisor`: per-account async runtimes, connection lifecycle, sync triggers, push stream consumption, status tracking.
- `supervisor.rs:351` main account runtime loop: startup sync, poll interval, automation backfill interval, cache maintenance interval, manual commands, push events.
- `supervisor.rs:917` `process_sync_trigger`: generates sync ID/span, sets progress, ensures gateway connection, calls `MailService::sync_account_with_mode`, publishes events and updates runtime overview; on failure records `sync.failed`, tears down gateway/push, updates status.
- `supervisor.rs:1095` `build_connection`: dispatches by `AccountDriver`.
  - Mock: `MockJmapGateway`.
  - JMAP: resolve secret, connect JMAP client, construct `LiveJmapGateway`, negotiate push transports (primary/fallback), wrap in resilient push stream.
  - IMAP/SMTP: resolve secret/OAuth access token, build IMAP/SMTP configs, discover/connect gateway with store, derive remote observation, optionally start one selected-mailbox IDLE event stream if IDLE available.
- `supervisor.rs:598-616` remote observation policy is provider/driver-specific. Push notifications trigger sync if changed IDs exist, checkpoint exists, or policy treats empty hints as sync-worthy.
- Runtime status changes append/publish `account.status_changed` events (`supervisor.rs:1453+`).

Spec gap: `docs/L1-api.md` endpoint table does not include `POST /auth/tokens`, `POST /accounts/{account_id}/oauth/start`, or `GET /sources/{source_id}/messages/{id}/attachments/{attachment_id}`, all of which exist in router/OpenAPI. It also says unknown `/v1` paths return API 404s, but this is router behavior rather than OpenAPI contract.

## Contracts, generated clients, and tests

- `crates/posthaste-server/src/openapi.rs:1-16` says Rust handlers/types are the source of truth. `ApiDoc` aggregates `#[utoipa::path]` handlers and schemas (`openapi.rs:198`). Served at `/v1/openapi.json`; committed artifact is repo-root `openapi.json`.
- `openapi.rs:203+` embeds repo-root `asyncapi.json` and serves it at `/v1/asyncapi.json`.
- `crates/posthaste-server/tests/openapi_contract.rs` compares generated OpenAPI to committed `openapi.json`; update via `UPDATE_OPENAPI=1 cargo test -p posthaste-server --test openapi_contract`.
- `crates/posthaste-server/tests/asyncapi_contract.rs` compares `asyncapi.json` event topic enum to `posthaste_domain::ALL_EVENT_TOPICS`.
- `crates/posthaste-server/tests/authz_completeness.rs` requires every non-exempt OpenAPI operation to have an authz map entry and vice versa.
- Web and MCP generated TS contracts both use `openapi-typescript ../../openapi.json`:
  - `apps/web/package.json`: `api:generate` -> `apps/web/src/api/schema.gen.ts`; `api:check` checks drift.
  - `apps/mcp/package.json`: `api:generate` -> `apps/mcp/src/schema.gen.ts`.
- High-value backend tests:
  - `crates/posthaste-server/tests/api_boundary_contracts.rs`: real store/config/service harness for API boundary behavior.
  - `provider_parity.rs`: JMAP vs IMAP projection/lazy body parity through shared `MailGateway`/`MailService`.
  - `stalwart_provider_parity.rs`: optional live Stalwart JMAP/IMAP/SMTP parity gated by `POSTHASTE_STALWART_INTEGRATION=1`.
  - `full_stack.rs`: Axum router + real store/auth tests for scoped data boundaries.
  - `auth_middleware.rs`, `capability_scoping.rs`, `settings_patch.rs`, `automation_*`, etc.
- Backend validation recipes: `just backend check` = logging contract + `cargo clippy --workspace --exclude posthaste-desktop --all-targets -- -D warnings`; `just backend test` = `cargo test --workspace --exclude posthaste-desktop`.

## Primary data flows to reflect in revised spec

### Startup/account runtime

1. Config repository loads TOML snapshot.
2. Server builds `DatabaseStore`, `MailService`, `AccountSupervisor`, routes.
3. Supervisor starts one runtime per enabled account.
4. Runtime builds provider gateway from account driver/transport/secrets.
5. Startup sync writes SQLite projections/events, then runtime loops on poll/push/manual/cache/backfill.

### Sync

```text
trigger (startup/poll/push/manual)
 -> supervisor ensures gateway + progress reporter
 -> MailService loads cursors from store
 -> gateway.sync(account, cursors) returns common SyncBatch
 -> DatabaseStore.apply_sync_batch transaction updates projections/cursors/events
 -> service post-commit cache candidate + automation work
 -> service appends sync.completed
 -> supervisor broadcasts events over SSE
```

Full-metadata mode removes message cursor for that cycle before calling gateway.

### Reads

```text
frontend/MCP/custom client -> /v1 REST -> API handler -> MailService -> DatabaseStore/config -> camelCase JSON
```

The frontend never reads SQLite or talks to providers directly.

### Mutations

```text
API command -> supervisor.gateway(account)
 -> MailService mutation method
 -> provider gateway command (JMAP ifInState or IMAP command path/local override)
 -> local MessageCommandStore/SyncWriteStore update + event(s)
 -> API returns CommandResult/OkResponse and publishes events where handler/supervisor does so
```

JMAP returns provider cursor for optimistic local cursor update. IMAP mutations generally return no cursor and rely on follow-up sync/location state.

### Lazy content/cache

- Message metadata sync is mandatory; bodies/raw/attachments are optional/lazy.
- Opening/fetching detail uses gateway body fetch then `apply_message_body`.
- Body cache worker in supervisor re-scores candidates and fetches through same gateway body path when budget allows.

## Current spec gaps / mismatches worth addressing

1. Missing/stale spec references: many comments reference `docs/eph/DESIGN-L1-*`, but no `docs/eph` directory exists in this workspace.
2. API endpoint table gaps vs router/OpenAPI: include `/auth/tokens`, `/accounts/{account_id}/oauth/start`, and attachment download route. Verify any other table drift against `openapi.json` rather than hand-maintaining.
3. `docs/L1-sync.md` schema list omits `message_attachment`; implementation has the table and attachment read/download paths.
4. `docs/L1-jmap.md` says `SearchSnippet/get` is used, but implementation search appears local smart-mailbox/query based; no local evidence found that JMAP `SearchSnippet/get` is currently used. Treat as future/aspirational unless verified.
5. Push architecture is two-layered: provider push/IDLE schedules sync in supervisor; API live updates to frontend are SSE from local `event_log`. Specs should keep those separate.
6. IMAP IDLE does not use `MailGateway::push_transports`; it is constructed directly in supervisor from discovery/capabilities.
7. OpenAPI is generated from Rust annotations and used as committed contract for TS clients; AsyncAPI is committed but only topic enum is drift-checked. Event payload schema drift may need more explicit validation if SPECial claims full generated event contract.
8. Domain comments still say “Core domain types and service logic for JMAP mail operations” in `posthaste-domain/src/lib.rs`, but implementation is now multi-driver (JMAP + IMAP/SMTP + mock). Spec wording should be provider-neutral.
