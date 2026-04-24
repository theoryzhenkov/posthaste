---
scope: L1
summary: "Logging contracts: crate layout, span conventions, config schema, event content rules, frontend logger interface"
modified: 2026-04-04
reviewed: 2026-04-24
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

The `tracing` ecosystem (`tracing`, `tracing-subscriber`, `tracing-appender`). All crates instrument with `tracing` macros; subscriber setup is centralized in `posthaste-server`.

### Subscriber stack

A layered subscriber composed of:

| Layer | Sink | Format | When |
|-------|------|--------|------|
| Stderr | terminal | Human-readable, ANSI colors | Always (dev and prod) |
| File appender | `<data_dir>/logs/posthaste-YYYY-MM-DD.log` | JSON lines (one object per event/span) | Always |
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

`RUST_LOG` env var takes precedence when set (development override). If the `tracing-subscriber` reload handle is straightforward to wire, level changes from config apply at runtime without restart. Otherwise, restart-to-apply is acceptable for v1.

### Log file management

- Rotation: daily, via `tracing-appender::rolling::daily`
- Retention: 7 days — older files deleted on rotation
- Location: `<data_dir>/logs/` (resolved from the existing config root directory logic)

### Span conventions

Spans use a `domain.operation` naming pattern with structured fields:

| Span | Fields | Crate |
|------|--------|-------|
| `http.request` | `method`, `path`, `status`, `latency_ms` | posthaste-server (tower-http) |
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
// that calls invoke("log_from_frontend", { level, domain, message }).
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
