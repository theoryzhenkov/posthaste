---
scope: L1
summary: "Logging contracts: crate layout, span conventions, config schema, frontend logger interface"
modified: 2026-04-03
reviewed: 2026-04-03
depends:
  - path: spec/L0-logging
  - path: spec/L1-accounts
    section: "Config schema"
  - path: spec/L1-api
    section: "Axum router"
---

# Logging — Contracts

## Rust backend

### Framework

The `tracing` ecosystem (`tracing`, `tracing-subscriber`, `tracing-appender`). All crates instrument with `tracing` macros; subscriber setup is centralized in `web-server`.

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
| `http.request` | `method`, `path`, `status`, `latency_ms` | web-server (tower-http) |
| `sync.cycle` | `account_id`, `mailbox_count`, `email_count` | mail-engine |
| `sync.method_call` | `method_name`, `state_token` | mail-engine |
| `push.connection` | `account_id`, `transport` (ws/sse), `attempt` | mail-engine |
| `push.event` | `event_type`, `changed_types` | mail-engine |
| `supervisor.action` | `account_id`, `action` | web-server |
| `store.query` | `operation`, `table` | mail-store |

Start coarse-grained (the above list). Add finer spans based on debugging experience.

### Error logging

Errors that cross crate boundaries (returned from trait methods) are logged at the boundary where they are handled, not where they originate. This avoids duplicate logging. Use `tracing::error!` with the error as a field: `error!(error = %e, "sync cycle failed")`.

## Frontend

### Library

`pino` — structured, leveled, browser-compatible.

### Interface

```typescript
import pino from "pino";

const logger = pino({
  level: import.meta.env.DEV ? "debug" : "info",
  browser: { asObject: true },
});

// Domain-scoped child loggers
const syncLogger = logger.child({ domain: "sync" });
const uiLogger = logger.child({ domain: "ui" });
```

### Log levels

Same semantics as the Rust side: `error`, `warn`, `info`, `debug`, `trace`.

### Sink

Browser console only for v1. The logger interface (`pino.Logger`) supports custom transports, so backend forwarding can be added later without changing call sites.

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
