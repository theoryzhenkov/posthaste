---
scope: L0
summary: "REST API + WebSocket boundary between Rust backend and web frontend"
modified: 2026-03-31
reviewed: 2026-03-31
depends:
  - path: README
  - path: spec/L0-jmap
dependents:
  - path: spec/L0-sync
  - path: spec/L0-ui
  - path: spec/L1-sync
  - path: spec/L1-ui
---

# API domain -- L0

## Why a REST API boundary

The Rust backend exposes an HTTP API that any frontend can consume -- a React SPA today, a Tauri desktop wrapper or native mobile app tomorrow. This replaces the previous UniFFI bridge approach, which was tightly coupled to Swift/macOS. The REST API is a strictly looser coupling at the cost of serialization overhead, which is negligible for a single-user local app running on localhost.

## Axum

The Rust web framework. Async, tower-based, well-maintained. It handles routing, JSON serialization, CORS, and serves the static frontend in production builds. In development, the frontend runs on Vite's dev server (port 5173) and the backend enables CORS for that origin.

## API design

RESTful JSON endpoints for reads. The frontend fetches data via standard HTTP GET requests. Mutations (move, delete, flag, send) use POST/PUT/DELETE. All responses use camelCase JSON keys (matching TypeScript conventions) even though Rust uses snake_case internally -- serde's `rename_all = "camelCase"` handles the translation.

## WebSocket for push

Planned, not yet implemented. A WebSocket connection at `/ws` will push real-time updates when the sync engine processes changes. This replaces the need for polling. The frontend subscribes on connect and receives events like `{ type: "emailsChanged", mailboxId: "..." }`. Until implemented, the frontend polls or uses React Query's refetch intervals.

## Rust owns everything

Unlike the previous architecture where Swift owned the database (GRDB), Rust now owns the entire backend: JMAP protocol, SQLite storage (rusqlite), sync engine, and API layer. The frontend is a stateless view that fetches and displays data. This simplifies the architecture significantly -- no FFI boundary, no callback interfaces, no dual-language cache ownership.

## Error handling

API errors are returned as JSON with HTTP status codes. 400 for bad requests, 404 for not found, 500 for internal errors. Each error response includes a `code` string and `message` string. Rust errors are mapped to API errors at the handler level; internal error details are not leaked to the frontend.

## Invariants

- The frontend never talks to JMAP directly; all server communication goes through the Rust backend
- All API responses use camelCase JSON
- Error responses are structured JSON with `code` and `message` fields
- The API is stateless from the frontend's perspective (all state lives in the Rust backend)
- CORS allows only the configured frontend origin(s)
