---
scope: L1
summary: "Logging contracts: crate layout, span conventions, config schema, event content rules, frontend logger interface"
modified: 2026-04-28
reviewed: 2026-04-28
depends:
  - path: docs/L0-logging
  - path: docs/L1-accounts
    section: "Config schema"
  - path: docs/L1-api
    section: "Axum router"
---

# Logging — Contracts

## Rust backend

### Framework

The `tracing` ecosystem (`tracing`, `tracing-subscriber`, `tracing-appender`). Application events use typed `posthaste-observability` wrappers over `tracing`; subscriber setup is centralized in `posthaste-server`.

### Subscriber stack

A layered subscriber composed of:

| Layer | Sink | Format | When |
|-------|------|--------|------|
| Stderr | terminal | Human-readable, ANSI colors | Always (dev and prod) |
| File appender | `<data_dir>/logs/posthaste.YYYY-MM-DD` | JSON lines (one object per event) | Always |
| Env filter | — | — | Controls which levels reach each layer |
| Reload filter | — | — | Enables runtime log-level changes |

### Log levels

| Level | Usage |
|-------|-------|
| `ERROR` | Unrecoverable failures, broken invariants |
| `WARN` | Degraded operation — retries, fallbacks, missing optional data |
| `INFO` | Lifecycle milestones — server start, sync complete, account connected, config loaded |
| `DEBUG` | Per-request details, JMAP method calls, state token changes |
| `TRACE` | Wire-level detail — raw payloads, SQL statements (opt-in) |

Default level: `INFO` for production, `DEBUG` for development (overridable via `RUST_LOG`).

### Configuration

Log level is a user-facing setting in the account/global TOML config:

```toml
[logging]
level = "info"          # One of: error, warn, info, debug, trace
```

`RUST_LOG` env var takes precedence when set (development override). Without `RUST_LOG`, the configured level applies to Posthaste crates only (`posthaste_server`, `posthaste_engine`, `posthaste_imap`, `posthaste_store`, `posthaste_domain`) while dependency crates default to `warn`. This keeps debug development logs useful without recording protocol-parser span noise from dependencies. If the `tracing-subscriber` reload handle is straightforward to wire, level changes from config apply at runtime without restart. Otherwise, restart-to-apply is acceptable for v1.

### Log file management

- Rotation: daily, via `tracing-appender::rolling::daily`
- Retention: 7 days — older files deleted on rotation
- Location: `<data_dir>/logs/` (resolved from the existing config root directory logic)
- Span close events are not emitted by default. Long-lived or high-frequency dependency spans can otherwise dominate JSON logs without adding operator-level diagnostics.

### Log querying

Dev stacks expose the active persisted log path through `just server-log-path` and follow it with `just server-log-tail`. Structured JSONL logs are queried with:

```sh
just server-log-query --account local-stalwart --message sync completed
just server-log-query --event http.request.completed --operation-kind mail.search
just server-log-query --sync-id 6f2a4a72-0c59-4d89-9d4e-2a2b9f2c4a87
just server-log-query --operation-id op_9fd8 --json --limit 50
just server-log-query --target posthaste_imap --json --limit 20
```

The query helper supports filtering by level, target substring, event name, account ID, sync ID, request ID, operation ID, operation kind, operation source, session ID, message substring, timestamp lower bound, and compact JSON output. Structured field filters check event fields plus active span fields so events emitted from nested spans still match.

### Log record schema

Application log events use a stable structured schema so local logs can later feed beta telemetry without reverse-engineering message text.

| Field | Required | Meaning |
|-------|----------|---------|
| `event` | SHOULD | Stable machine-readable event name. Dot-separated lower-case namespaces, e.g. `http.request.completed`. |
| `message` | MUST | Human-readable summary. The message may change for clarity and must not be used as the primary analytics key. |
| `target` | MUST | Rust module target or `frontend` for frontend-forwarded logs. |
| `level` | MUST | Severity emitted by the subscriber (`ERROR`, `WARN`, `INFO`, `DEBUG`, `TRACE`). |
| `error` | SHOULD on failures | Sanitized error summary. Include operational context as separate fields. |
| `*_id` | SHOULD where applicable | Stable non-secret identifiers such as `account_id`, `request_id`, `operation_id`, `sync_id`. |
| `*_count` | SHOULD for counts | Integer counts, e.g. `message_count`, `event_count`, `failed_count`. |
| `*_bytes` | SHOULD for byte sizes | Integer byte counts, e.g. `fetch_byte_limit`, `cached_bytes`. |
| `*_ms` | SHOULD for durations | Integer milliseconds, e.g. `latency_ms`, `duration_ms`, `elapsed_ms`. |

Event names describe what happened, not where it was logged: `<domain>.<object>.<outcome>` or `<domain>.<operation>.<stage>`. Examples: `api.request.started`, `api.request.completed`, `http.request.completed`, `cache.maintenance.completed`, `sync.cycle.failed`. New user-facing or telemetry-relevant logs must include `event`.

Rust application events are emitted through `posthaste-observability` typed macros such as `ph_info!(events::HTTP_REQUEST_COMPLETED, ...)`. The `LogEvent` constructor is private, so new event names must be added to the shared registry before code can compile against them. Raw `tracing::{trace,debug,info,warn,error}!` event macros are rejected by the Rust logging contract check wired into backend build/check recipes.

Frontend application events are emitted through typed logger instances from `src/logger.ts`. Their first argument must include `event: LogEvent`, where `LogEvent` comes from `src/logEvents.ts`. The frontend logging contract check rejects direct `pino` imports outside the logger adapter and event string literals outside the event registry.

### Noise budget

Logs should record decisions and outcomes, not every idle poll. Periodic background workers should avoid DEBUG events when no work was found, unless the event has an operation ID, reports an error, or changes a rate/backoff/resource decision. Candidate-level cache details and protocol wire details belong at TRACE.

### Correlation metadata

Local logs must carry stable, non-secret identifiers that make one user action queryable across the webview, HTTP boundary, backend handlers, and background work it triggers.

| Field | Meaning |
|-------|---------|
| `request_id` | One HTTP request. Generated by the frontend API client; generated by the backend if absent. |
| `operation_id` | One user-facing operation, such as one message-list load or search preview. Reused by every HTTP request produced by that operation. |
| `operation_kind` | Low-cardinality operation name such as `mail.list`, `mail.search`, or `mail.search.preview`. |
| `operation_source` | UI or subsystem that created the operation, such as `message-list.smart-mailbox` or `command-palette`. |
| `session_id` | One frontend observability session, stored in browser session storage. |
| `process_id` | Operating-system process ID for the emitting process. |
| `process_role` | Stable process role such as `backend` or `webview`. |

The frontend sends correlation metadata to the backend on API calls using these headers: `X-PostHaste-Request-Id`, `X-PostHaste-Operation-Id`, `X-PostHaste-Operation-Kind`, `X-PostHaste-Operation-Source`, and `X-PostHaste-Session-Id`. The backend accepts only short ASCII token values for these fields before recording them in logs. The desktop IPC log bridge applies the same rule to frontend-forwarded `event` and correlation fields.

### Span conventions

Spans use a `domain.operation` naming pattern with structured fields:

| Span | Fields | Crate |
|------|--------|-------|
| `http.request` | `method`, `path`, `status`, `latency_ms`, `request_id`, `operation_id`, `operation_kind`, `operation_source`, `session_id`, `process_id`, `process_role` | posthaste-server (tower-http) |
| `sync.cycle` | `account_id`, `mailbox_count`, `email_count` | posthaste-engine |
| `sync.method_call` | `method_name`, `state_token` | posthaste-engine |
| `push.connection` | `account_id`, `transport` (ws/sse), `target_url`, `attempt` | posthaste-engine |
| `push.event` | `event_type`, `changed_types` | posthaste-engine |
| `supervisor.action` | `account_id`, `action` | posthaste-server |
| `store.query` | `operation`, `table` | posthaste-store |

Start coarse-grained (the above list). Add finer spans based on debugging experience.

### Event content rules

Spans and span conventions define *where* instrumentation lives. This section defines *what* each event must contain so that failures are diagnosable from logs alone, without attaching a debugger or adding ad-hoc print statements.

The guiding test: given a failure in production, can an operator identify *what* failed, *where* the request went, and *what the system decided* by reading the logs at INFO level? If not, the instrumentation is incomplete.

#### Connection events

Any event that opens a network connection (HTTP, WebSocket, SSE, TCP) must include the **target URL or host:port** as a structured field. This applies at both initial connection and reconnection.

When a connection attempt fails, the target must appear in the WARN/ERROR event. The error message alone ("400 Bad Request") is not sufficient — the target tells the operator *where* to look.

When a connection succeeds, log the target at INFO alongside any negotiated parameters (protocol version, supported capabilities).

#### Capability and negotiation events

When the system discovers capabilities from an external server (JMAP session, WebSocket support, push support), the discovered values must be logged at INFO. This includes URLs, feature flags, and version identifiers that influence subsequent behavior.

When the system makes a transport or strategy decision based on those capabilities (e.g. "use WS as primary, SSE as fallback"), log the decision and the inputs that drove it. Silent negotiation makes failures look like they come from nowhere.

#### Retry and fallback events

Events within a retry or fallback loop must include the **attempt number** and the **threshold** that triggers the next stage. A WARN event saying "push transport open failed" should read `attempt=2 fallback_threshold=3` so the operator knows how close the system is to escalating.

When a fallback fires, log both the transport being abandoned and the transport being tried, along with the cumulative failure count.

When backoff delays are applied, log the delay duration. This prevents confusion about apparent inactivity in the logs.

#### Lifecycle transitions

State machine transitions that change an account or subsystem's operational status (Offline -> Syncing, Connected -> Reconnecting, Primary -> Fallback) must produce an INFO-level event. The event must include the previous state and the new state, not just the new state.

### Error context preservation

Errors that cross crate or module boundaries (returned from trait methods, mapped through error enums) lose context at each mapping step. A raw `jmap_client::Error` carries connection details; by the time it becomes `GatewayError::Network(String)`, only the message survives.

The rule: when mapping an error at a boundary, attach the **operational context** that the caller has but the error doesn't. This means the site that called the failing function adds its own fields, since it knows the target URL, account ID, transport type, or method name that the lower-level error cannot.

In practice, this means the `warn!` or `error!` event at the handling boundary includes both the mapped error *and* the contextual fields as structured tracing fields, not concatenated into the error string.

Do not embed operational context (URLs, account IDs) into error variant payloads. Errors describe *what* went wrong; the log event at the handling boundary describes *where* and *during what*.

### Sensitive data rules

Log events must never contain credentials, tokens, passwords, or session secrets at any level. This is not a level-gating rule — these values are banned from log output entirely.

Email body content (HTML or plain text) must not appear in logs. Body-related log events use metadata only: message ID, content type, byte length.

Email addresses may appear at DEBUG and below for debugging sync and delivery flows. At INFO and above, use account ID or mailbox ID as the identifier.

Raw JMAP request/response payloads may appear at TRACE only. DEBUG-level method call events use structured fields (method name, object count, state token) rather than serialized JSON.

### Error logging

Errors that cross crate boundaries (returned from trait methods) are logged at the boundary where they are handled, not where they originate. This avoids duplicate logging. Use `tracing::error!` with the error as a field: `error!(error = %e, "sync cycle failed")`.

## Frontend

### Library

`pino` — structured, leveled, browser-compatible.

### Interface

```typescript
import pino from "pino";
import { invoke } from "@tauri-apps/api/core";

// In Tauri, pino's browser.write is replaced with a custom handler
// that calls invoke("log_from_frontend", {
//   level,
//   domain,
//   message,
//   event,
//   requestId,
//   operationId,
//   operationKind,
//   operationSource,
//   sessionId,
// }).
// In browser-only mode, falls back to { asObject: true } (console).
const logger = pino({
  level: import.meta.env.DEV ? "debug" : "info",
  browser: isTauri() ? makeTauriWrite() : { asObject: true },
});

// Domain-scoped child loggers — the domain field flows through IPC
const syncLogger = logger.child({ domain: "sync" });
const uiLogger = logger.child({ domain: "ui" });
const apiLogger = logger.child({ domain: "api" });
```

### Log levels

Same semantics as the Rust side: `error`, `warn`, `info`, `debug`, `trace`.

### Sink

In Tauri (desktop), pino logs are forwarded to the Rust tracing subscriber via a `log_from_frontend` IPC command, landing in the same daily-rotated JSON log files as backend events. The `domain` field from pino child loggers is preserved as a structured field with `target: "frontend"`.

WebKit console output (`console.log/info/debug/warn/error`) is also captured and forwarded with `domain: "webview"`, catching logs from React, third-party libraries, and uncaught errors. Pino log objects are detected and skipped in the console capture to avoid double-sending.

Frontend pino logs may include `event`, `requestId`, `operationId`, `operationKind`, `operationSource`, and `sessionId`. The IPC bridge preserves `event` and maps those camelCase fields into the backend log fields `request_id`, `operation_id`, `operation_kind`, `operation_source`, and `session_id`; missing fields are recorded as empty strings.

In browser dev mode (no Tauri), pino logs go to the browser console only. In Tauri dev mode, logs go to both the console (for devtools) and the backend.

## Assertions

| ID | Sev. | Assertion |
|----|------|-----------|
| stderr-human | MUST | Stderr output uses human-readable format with ANSI colors |
| file-json | MUST | File output uses JSON-lines format, one object per line |
| daily-rotation | MUST | Log files rotate daily with filenames containing the date |
| seven-day-retention | SHOULD | Log files older than 7 days are deleted automatically |
| config-level | MUST | Log level is configurable via `[logging].level` in TOML config |
| env-override | MUST | `RUST_LOG` env var overrides config-file level when set |
| app-scoped-default-filter | SHOULD | Without `RUST_LOG`, configured debug/trace levels apply to Posthaste crates while dependency crates stay at warn-or-higher |
| query-helper | SHOULD | Dev tooling provides a JSONL log query helper with account, sync ID, target, level, message, and since filters |
| query-helper-event | SHOULD | The JSONL log query helper can filter by stable `event` name |
| query-helper-correlation | SHOULD | The JSONL log query helper can filter by request ID, operation ID, operation kind, operation source, and session ID |
| stable-event-field | SHOULD | New telemetry-relevant log events include a stable dot-separated `event` field separate from human `message` |
| typed-log-events | MUST | Application log calls use typed event constants/unions rather than ad-hoc event strings |
| log-contract-checks | MUST | Backend build/check and frontend typecheck/build reject raw logging APIs that bypass the typed event contract |
| request-correlation | MUST | HTTP request logs include `request_id`, generated by the frontend when possible and by the backend otherwise |
| operation-correlation | MUST | User-facing frontend operations propagate `operation_id`, `operation_kind`, and `operation_source` through API requests and related backend logs |
| process-metadata | SHOULD | Backend and frontend-forwarded logs include `process_id` and `process_role` |
| search-cache-operation | SHOULD | Search-triggered cache maintenance logs preserve the operation ID that triggered the work |
| noise-budget | SHOULD | Periodic background workers avoid DEBUG logs for idle no-op ticks unless an operation ID, error, or resource decision changed |
| coarse-spans | MUST | Spans exist at HTTP request, sync cycle, push connection, and store query boundaries |
| fe-pino | MUST | Frontend uses pino with domain-scoped child loggers |
| error-boundary | SHOULD | Errors are logged at the handling boundary, not the origination point |
| conn-target | MUST | Connection attempt and failure events include the target URL or host:port |
| capability-log | MUST | Discovered server capabilities (URLs, feature flags) are logged at INFO on session establishment |
| negotiation-log | MUST | Transport selection decisions are logged with the chosen transport, alternatives, and rationale |
| retry-state | MUST | Retry/fallback events include attempt number and threshold |
| error-context | SHOULD | Error-handling log events include operational context (target, account, transport) as structured fields, not embedded in the error string |
| no-secrets | MUST | Credentials, tokens, passwords, and session secrets never appear in log output at any level |
| no-body-content | MUST | Email body content (HTML or plain text) never appears in log output |
| pii-level-gate | SHOULD | Email addresses appear only at DEBUG and below; INFO and above use account/mailbox IDs |
| payload-trace-only | SHOULD | Raw JMAP request/response payloads appear only at TRACE level |
| fe-tauri-bridge | MUST | In Tauri, pino logs are forwarded to the Rust tracing subscriber via the `log_from_frontend` IPC command |
| fe-webview-capture | MUST | In Tauri, WebKit console output is captured and forwarded with `domain: "webview"` |
| fe-no-double-send | MUST | Pino log objects routed through console in dev mode are not re-forwarded by the console capture |
| fe-browser-fallback | MUST | In browser-only mode (no Tauri), pino logs go to the browser console without errors |
