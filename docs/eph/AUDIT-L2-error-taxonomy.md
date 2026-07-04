---
scope: L2
summary: "Error-taxonomy audit — full type census, conversion edge list with information-loss flags (the M29/D70-D73 worklist input), retryability findings (the sole typed retry signal is written but never read), swallowing/panic census, boundary mappings, top-10. Evidence from the pre-M9c snapshot; paths flagged; transport.rs edges need re-verify against the near-end engine."
modified: 2026-07-03
reviewed: 2026-07-03
lifecycle: ephemeral
type: AUDIT
state: evidence-complete
depends:
  - path: eph/RFC-L2-lifecycle-and-errors
dependents: []
---

# Error-Taxonomy Audit — Consolidated Report

> **Status: EVIDENCE/AUDIT (evidence-complete).** Read-only investigation, not a
> plan. This is the M29/D70–D73 worklist input for RFC-L2-lifecycle-and-errors —
> those decisions shipped (M29 landed); the findings here are preserved as
> provenance. Paths are pre-M9c snapshot (renames noted in the provenance note).

**Provenance note:** all evidence gathered from the pre-M9c snapshot worktree `/home/usr.prj_posthaste/src/.workspaces/architecture-cleanup-hygiene/` (paths below are relative to it). Post-M9 renames apply: link-core→replica-core, link-replica→replica-projector, link-wasm→client-node-wasm, RuntimeSessionId→RuntimeLinkId; the near-end engine now owns transport error classification at both seams — findings against `runtime/src/transport.rs` should be re-verified against the new owner. The HTTP-boundary sweep was delivered separately to the orchestrator; this report covers that seam only via my directly-read evidence (`api/errors.rs`).

## 1. Error-type census

| Type | Definition | Constructed by | Consumed by |
|---|---|---|---|
| `GatewayError` (7 variants: Unavailable, Auth, Network, StateMismatch, CannotCalculateChanges, Rejected, MutationRejected{readback, reason}) | domain-model/src/model/errors.rs:7-31 | imap gateway classifier (imap/src/gateway/utils.rs:21-51), engine live paths (engine/src/live_mutation/requests.rs:98-126, live_compose/*, push_common.rs:66), domain-service payload codecs (domain-service/src/service.rs:107,122) | `ServiceError::Gateway` via `#[from]` |
| `StoreError` (NotFound, Conflict, Failure, Corruption) | errors.rs:37-50 | store crate throughout (store/src/db/connection.rs:72-99 wraps rusqlite/io/serde; commands.rs:143-192; outbox.rs; rev_log.rs; smart_mailboxes/*) | `ServiceError::Store`; also collapsed by `store_error_to_gateway` (imap/src/gateway/utils.rs:49-51) |
| `SecretStoreError` (Unavailable, Unsupported) | errors.rs:156-161 | secret-store impls | `ServiceError::Secret` |
| `ConfigError` (NotFound, Conflict, Validation, Io, Parse — all `String` payloads) | domain-model/src/config.rs:9-20 | config repository (config/src/repository/*; poison→`Io` at repository/io.rs:257) | `ServiceError::Config` |
| `ValidationError` (8 variants) | domain-model/src/validation.rs:4-13 | config/domain validation | `ConfigError::Validation` (stringified), `RuntimeError` (typed-ish) |
| `ServiceError` (sum of the above) + `ServiceErrorKind` (15-way copy enum, `.kind()` at errors.rs:115-142, stable `.code()` strings) | errors.rs:56-109 | domain-service | http-api-adapter (`ApiError`), contract-core (`RuntimeError`) |
| `ImapAdapterError` (25 variants, rich structure: UidValidityMismatch{expected,actual}, MissingAttachment{...}, etc.) | imap/src/error.rs:6-58 | imap crate; 3 `From` impls stringify library errors (error.rs:60-76) | `imap_error_to_gateway` only |
| `RuntimeAdapterError` {code, message, retryable, correlation_id, details} + `RuntimeErrorCode` (26 variants) + `RuntimeError` newtype | contract-core/src/lib.rs:791-920 | ctors at lib.rs:844-952 (`new`/`with_details`/`internal` default `retryable:false`; only `retryable()` sets true); dozens of `Internal` catch-alls (see edges) | runtime-api + client-link + authority-server-link traits (all `Result<_, RuntimeError>`, runtime-api/src/lib.rs:42+, client-link/src/lib.rs:57+); wire frames `ViewError`/`ViewSnapshot.error`/`MutationReceipt.error`/`MutationNotification::Rejected` (lib.rs:384,672,734,764); `ApiError::from_runtime_error` |
| `LinkError` {status, body{code:&'static str, message, details}} — **no retryable, no correlation_id** | authority-server/src/link_wire.rs:125-136 | `from_runtime_error` (link_wire.rs:140-156) + `runtime_error_status` (181-222) | `IntoResponse` (171-175) → link-wire HTTP |
| `EngineError` {disposition, message, status, error:Option<RuntimeAdapterError>} + `Disposition` (Permanent/Transient) + `TransportError{message}` | link-near-end/src/engine.rs:44-84, error.rs:11-35, transport.rs:57-65 | `from_response` re-parses 4xx body to typed envelope (engine.rs:72-84) | engine retry loop → `ConnectionStatus::{Transient,Permanent}Error(String)` (sink.rs:14-26) |
| `SettlementOutcome`/`WireSettlementOutcome` (Confirmed/Failed — **no error payload**) | link-core/src/convergence.rs:57-66; authority-server-link/src/lib.rs:179-202 | authority-server commands | replica settlement |
| `ApiError`/`ApiErrorCode` (37 codes)/`ApiErrorBody{code,message,details}` | http-api-adapter/src/api/errors.rs:13-101 | handlers + the two From-paths | `/v1` JSON responses |
| TS `ApiError` {status, statusText, message, code} | apps/web/src/api/errors.ts:7-24, parsed at api/client/core.ts:69-85 | `parseError` | notification center, components |

Peripheral (out of core flow): `TokenError` (adapter/src/token.rs:191), `RuntimeBuildError`/`RuntimeShutdownError` (runtime/src/shutdown.rs:34,54), `LabError`, `FetchError`, `FixtureError`.

## 2. Conversion edge list (⚠ = information loss)

```
imap_client::ClientError      --From, imap/error.rs:60-64------------------> ImapAdapterError::Client(String)      ⚠ source→string
lettre::Error / smtp::Error   --From, imap/error.rs:66-76------------------> ImapAdapterError::{BuildSmtpMessage,Smtp}(String) ⚠ source→string
ImapAdapterError (20 variants)--imap_error_to_gateway, imap/gateway/utils.rs:21-41 --> GatewayError::Rejected(String) ⚠⚠ 20→1, all structure (UidValidityMismatch etc.) flattened to Display
ImapAdapterError::Auth        --utils.rs:42--------------------------------> GatewayError::Auth                     (message dropped ⚠)
ImapAdapterError::{Client,Smtp}--utils.rs:43-45----------------------------> GatewayError::Network(String)
StoreError (ALL incl. Corruption) --store_error_to_gateway, utils.rs:49-51-> GatewayError::Rejected(String)         ⚠⚠ Corruption mislabeled as client rejection
rusqlite::Error               --sql_to_store_error, store/db/connection.rs:72-78 --> StoreError::{Corruption|Failure}(String) (corruption preserved; else string)
io/serde errors               --connection.rs:92-99------------------------> StoreError::Failure(String)            ⚠ kind erased
reqwest/serde in engine       --live_mutation/requests.rs:98-126-----------> GatewayError::Network(String)          ⚠ serde decode error labeled Network
serde_json in domain-service  --service.rs:107,122 (encode/decode_payload)-> GatewayError::Rejected(String)         ⚠ internal serialization bug labeled gateway-rejected
GatewayError/StoreError/SecretStoreError/ConfigError --#[from], errors.rs:57-64 --> ServiceError  (lossless wrap)
ValidationError               --From, config.rs:22-32----------------------> ConfigError::Validation(String)        ⚠ variant→string; Vec joined with "; "
ValidationError               --From, contract-core/lib.rs:954-973---------> RuntimeError                           ⚠ 3 variants collapse to InvalidAccount; retryable=false; details=Null
ServiceError                  --From, contract-core/lib.rs:975-996---------> RuntimeError                           ⚠⚠ 15→15 codes OK, but retryable ALWAYS false (even NetworkError/GatewayUnavailable); source→Display string; MutationRejected.readback lost
ServiceErrorKind              --From, api/errors.rs:57-77------------------> ApiErrorCode  (1:1, lossless)
ServiceError                  --ApiError::from_service_error, api/errors.rs:107-117 --> ApiError  ⚠ message = error.to_string() leaked verbatim; details={}
RuntimeError                  --ApiError::from_runtime_error, api/errors.rs:122-133 --> ApiError  ⚠ retryable+correlation_id dropped; envelope.message leaked verbatim
RuntimeError                  --LinkError::from_runtime_error, link_wire.rs:140-222 --> LinkError ⚠ retryable+correlation_id dropped; RuntimeNotReady/TransportDisconnected/Internal→"internal_error"; InvalidDescriptor/InvalidMutation→"invalid_query"
LinkError (remote HTTP body)  --RemoteAuthorityServer::post_link, runtime/transport.rs:89-96 --> RuntimeError::new(GatewayRejected, "…({status}): {body}") ⚠⚠⚠ typed body NOT parsed; every remote error becomes non-retryable GatewayRejected string blob (pre-M9 path; classification since moved to near-end engine — re-verify)
subscribe non-2xx             --runtime/transport.rs:229-234---------------> RuntimeError::retryable(TransportDisconnected) ⚠ body dropped
reqwest::Error                --transport_error, runtime/transport.rs:103-108 --> RuntimeError::retryable(TransportDisconnected, String)
4xx HTTP body                 --EngineError::from_response, link-near-end/engine.rs:72-84 --> EngineError{error:Some(typed)} ⚠ disposition re-derived from status band; envelope.retryable ignored
EngineError                   --engine.rs:266-302--------------------------> ConnectionStatus::{Transient,Permanent}Error(String) ⚠ typed envelope → string
MutationReceipt{state,error}  --near_node.rs:82-89-------------------------> bool confirmed                          ⚠ tolerable (error already on notification frame)
StoredMutation                --far_end/sessions.rs:94-106-----------------> MutationNotification::{Confirmed|Rejected{error}} (preserved; synthesizes Internal if None)
anyhow/typed misc (~20 sites) --authority-server/{authority_server.rs:111, mutations.rs:182, mutations/accounts.rs:29,65,183, mutations/smart_mailboxes.rs:9,52, reads.rs:265, commands.rs:78,96,481,534}, runtime/{views.rs:103-156, handle.rs:105,142, read.rs:510} --> RuntimeError Internal(String) ⚠ catch-all collapse
SettlementOutcome <--From/From--> WireSettlementOutcome (authority-server-link:186-202)  lossless, BUT carries no error payload at all
wire ApiErrorBody             --TS parseError, api/client/core.ts:69-85----> ts ApiError ⚠ `details` never read (typed Record<string,never> in schema.gen.ts:1309)
```

## 3. Retryability

- **Data:** exactly one typed signal — `RuntimeAdapterError.retryable: bool` (contract-core:794). `RuntimeErrorCode` has no terminal/retryable grouping. Set `true` in only 3 places (contract-core:863; runtime/transport.rs:104,231). Every other ctor and both `From` impls default `false`.
- **Readers: zero in production.** Rust: only a test assertion (contract-core:1036). TS: logged once (entityStoreAdapter.ts:697-708), never branched on.
- **Convention instead:** near-end engine decides by HTTP status band (`classify_status`, link-near-end/error.rs:29-35: 4xx=Permanent, else=Transient), overriding the envelope's own flag (engine.rs:79). TS `httpAdapter.ts:269-273` likewise: 4xx=fatal, 5xx=reconnect-forever (sessionClient.ts:46-56). React-Query retries everything once regardless (queryClient.ts:16-21). Backoff loops (authority-server/push.rs:27-154, domain-service/cache/resource_governor.rs:99-118) never inspect error codes.
- **Erasure:** remote hop drops retryability twice — `LinkErrorBody` has no field (link_wire.rs:130-136) and `post_link` maps everything to non-retryable `GatewayRejected` (transport.rs:89-96). `From<ServiceError>` marks transient `NetworkError`/`GatewayUnavailable` as non-retryable (lib.rs:994).

## 4. Swallowing census (top items; full list in sweep)

- `let _ = process_sync_trigger/... ` × 7 (authority-server/supervisor/runtime.rs:48-342) — sync/backfill cycle result discarded; a persistently failing account silently never progresses.
- `let _ = fs::set_permissions(state_root, 0o700)` (server/main.rs:152) — **security**: secrets dir may stay world-readable, no warning.
- `let _ = outbox.remove_operation(...)` (domain-service/service/mutation.rs:33) — failed compensation leaves orphaned queued op.
- `verify_account`: `fetch_identity(...).await.ok()` (authority-server/supervisor/manager.rs:261) — probe error → `ok:true, identity:None`.
- rev-log `unwrap_or(Value::Null)`/`unwrap_or_default()` (store/rev_log.rs:29,163) — corrupt undo/redo rows silently become null diffs.
- SSE serialization `unwrap_or_else(|_| Event::default().data("{}"))` × 4 (link_wire.rs:472; adapter cursor_support.rs:53, runtime_stream.rs:67, views.rs:112) — client silently gets `{}`.
- Readback `.ok()` (engine/live_mutation.rs:45,152) — settlement proceeds with `None` readback; silent drift.
- TS: `discardOperation/retryOperation(...).catch(() => {})` (OutboxPane.tsx:77,83) — user's Retry click can fail invisibly; `viewError` frames dropped with bare `return` (useRuntimeMailListView.ts:254-259, useRuntimeObjectView.ts:85-90, useRevLogMirror.ts:79-84).

## 5. Panic policy

Zero bare `.unwrap()` in production. ~45 `.expect()`: mostly boot fail-fast (acceptable) plus documented invariants (store.rs:47,125; token.rs:182-272; build.rs:416). Reachable-by-input bugs: `panic!` on `require_auth=true` with empty `[link].runtimes` (link_wire.rs:105); TLS/CORS config parse panics at serve.rs:148,152. Poisoned-mutex: three policies coexist — panic-on-poison (runtime/read.rs:89-280, near_node.rs:67, runtime_registry.rs ×7, account_repository.rs ×11), recover-with-warn (store/store.rs:63-79), map-to-`ConfigError::Io` (config repository, io.rs:257).

## 6. Boundary behavior

**ServiceErrorKind → HTTP** (api/errors.rs:214-231): NotFound→404; Conflict,StateMismatch→409; AuthError→401; GatewayUnavailable→503; NetworkError→502; GatewayRejected,Secret*,ConfigValidation,ConfigParse→400; CannotCalculateChanges,StorageFailure,StorageCorrupted,ConfigIo→500.
**RuntimeErrorCode → HTTP** (api/errors.rs:148-212): RuntimeNotReady→503/InternalError; InvalidDescriptor,InvalidMutation→400/InvalidQuery; Unauthorized→401; NotFound→404; ProviderUnavailable→503; Conflict,StateMismatch→409; NetworkError→502; TransportDisconnected,Internal→500/InternalError; rest→400 or 500 per code.
**Message leaks:** `error.to_string()` / `envelope.message` flow verbatim into `ApiErrorBody.message` (errors.rs:113,129) and are rendered verbatim in ~8 TS surfaces (NotificationsPanel.tsx:120; MessageList.tsx:362; TagsPane.tsx:78; OutboxPane.tsx:105-108; ErrorBoundary.tsx:95; …). Since `StoreError::Failure` carries raw rusqlite/io text, **SQL/filesystem error strings can reach the UI**.

## 7. Logging vs surfacing

Invisible-to-user: daemon event-stream death (warn-only, useDaemonEvents.ts:89-106); `viewError` frames (no log, no UI); loadMore failure (log-only despite "must not be silent" comment, useRuntimeMailListView.ts:331-344); attachment download (logged then swallowed, MessageAttachments.tsx:169-171). Surfaced-without-logging: ThemeProvider toast (ThemeProvider.tsx:95-103). Rust sync-cycle `let _` sites are invisible to operators except side-band status.

## Top 10 by user-visible severity

1. **Remote link hop stringifies all errors to non-retryable `GatewayRejected`** (runtime/transport.rs:89-96): a far-node 503 during a mailbox move arrives as terminal rejection — user sees "gateway rejected" for a transient outage; co-located deployment behaves differently. (Pre-M9 path — re-verify against new engine owner.)
2. **`retryable` flag written, never read anywhere** (only readers: contract-core:1036 test, one TS log line) — the system's sole typed retry signal is dead; all retry is status-band guessing.
3. **`viewError` frames silently dropped in all three TS handlers** — mid-stream terminal error leaves stale mail rows on screen forever, no log, no UI.
4. **`store_error_to_gateway` folds `StoreError::Corruption` into `Rejected`** (imap/gateway/utils.rs:49-51) — DB corruption during IMAP ops loses its repair-pathway classification, surfaces as a 400-class rejection instead of `storage_corrupted` (which TS specifically handles, notifyFromError.ts:31).
5. **`From<ServiceError> for RuntimeError` hard-codes `retryable:false`** (contract-core:994) — transient NetworkError/GatewayUnavailable become terminal at the contract seam.
6. **Sync-cycle results discarded** (supervisor/runtime.rs ×7) — an account that can never sync fails silently; user just sees stale mail.
7. **Internal error strings leak verbatim to the UI** (errors.rs:113/129 → ~8 TS render sites) — raw SQLite/IO text in the notification center.
8. **5xx stream failure → infinite 1s reconnect** (httpAdapter.ts:274,299-303 + sessionClient.ts:46-56), warn-only — permanent server fault looks like "still loading" forever.
9. **Outbox Retry/Discard failures swallowed** (OutboxPane.tsx:77,83) — the recovery affordance itself can fail with zero feedback.
10. **20 `ImapAdapterError` variants collapse to `GatewayError::Rejected(String)`** (utils.rs:21-41) — UIDVALIDITY mismatch (needs resync) is indistinguishable from a bad SMTP address (needs user edit); everything downstream is string-shaped.

Raw per-sweep materials (swallowing/panic census, TS boundary detail, link-seam census) are in the three sub-agent reports above; the HTTP-boundary sweep went directly to the orchestrator. This edge list is the M29 worklist input.
