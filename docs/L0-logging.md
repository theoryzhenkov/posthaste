---
scope: L0
summary: "Production-grade structured logging and tracing across Rust backend and React frontend"
modified: 2026-04-04
reviewed: 2026-04-24
depends:
  - path: docs/L0-api
  - path: docs/L0-sync
  - path: docs/L0-accounts
dependents:
  - path: docs/L1-logging
---

# Logging

Structured, leveled logging and tracing for PostHaste. Replaces ad-hoc `println!` output with production-grade observability across both the Rust backend and the React frontend.

## Why

- **Debugging pain**: Async flows across JMAP sync, push transport, and Axum handlers are opaque with `println!`.
- **Production readiness**: A daily-driver mail client needs persistent, rotated logs for post-mortem analysis.
- **Observability**: Span-based tracing reveals where time is spent and where failures occur in the sync pipeline.

## Scope

In scope:

- `tracing` ecosystem on the Rust backend (structured spans, leveled events, multiple subscribers)
- `pino` structured logger on the React frontend
- Dual-sink output: human-readable stderr (dev) + JSON-lines log files (production)
- Daily log rotation (7-day retention cleanup deferred — see L1 assertion `seven-day-retention`)
- Per-request HTTP tracing via `tower-http::TraceLayer`
- Log level as a user-facing setting in the TOML config, runtime-reconfigurable if feasible
- Priority instrumentation of JMAP sync and push transport
- Frontend-to-backend log forwarding via Tauri IPC (pino logs and WebKit console output routed to the Rust tracing subscriber)

Out of scope (for now):

- Prometheus metrics or OpenTelemetry export
- Crash / panic reporting service
- Fine-grained SQL query tracing
- In-app log viewer UI
