---
scope: L0
summary: "REST API + SSE boundary between Rust backend and web frontend"
modified: 2026-04-01
reviewed: 2026-04-01
depends:
  - path: README
  - path: docs/L0-jmap
dependents:
  - path: docs/L0-sync
  - path: docs/L0-ui
  - path: docs/L1-api
  - path: docs/L1-sync
  - path: docs/L1-ui
---

# API domain -- L0

## Why a REST API boundary

The Rust backend exposes an HTTP API that any frontend can consume: a React SPA today, a Tauri wrapper or native mobile client later. This replaces the previous UniFFI bridge approach, which was tightly coupled to Swift/macOS. The REST boundary is a looser coupling at the cost of serialization overhead, which is negligible for a single-user local app running on localhost.

## Axum

Axum handles routing, JSON serialization, CORS, and serving the static frontend in production builds. In development, the frontend runs on Vite's dev server and the backend enables CORS for that origin.

## API design

RESTful JSON endpoints serve reads. The frontend fetches sidebar data, message detail, and conversations through ordinary HTTP requests. Mutations such as move, delete, flag, and manual sync use POST or DELETE. All responses use camelCase JSON keys even though Rust uses snake_case internally.

The main list surface is conversation-first rather than raw-message-first:

- `/v1/views/conversations`
- `/v1/smart-mailboxes/{id}/conversations`
- `/v1/views/conversations/{conversationId}`

List endpoints support cursor pagination with `limit` and `cursor`. The cursor is opaque to the client and encodes the seek position used by the backend's sort order.

## Server-Sent Events for push

Push is implemented with EventSource at `/v1/events`, not WebSocket. The backend writes ordered domain events into `event_log`, publishes them through Axum, and the frontend reconnects with `afterSeq` so it can resume from the last seen event without replaying the full history. This keeps push one-way and simple, which matches the frontend's needs: invalidation, list refresh, and message-detail refresh.

## Rust owns everything

Unlike the previous architecture where Swift owned the database, Rust now owns the entire backend: JMAP protocol, SQLite storage, sync engine, event log, and API layer. The frontend is a stateless consumer of API data plus local interaction state. This removes the FFI boundary and the risk of split cache ownership.

## Error handling

API errors are returned as JSON with HTTP status codes. 400 for bad requests, 404 for not found, 500 for internal errors. Each error response includes a `code` string and `message` string. Rust errors are mapped to API errors at the handler level; internal error details are not leaked to the frontend.

## Invariants

- The frontend never talks to JMAP directly; all server communication goes through the Rust backend
- All API responses use camelCase JSON
- Error responses are structured JSON with `code` and `message` fields
- Live updates flow through SSE at `/v1/events`, with `afterSeq` resume support
- Conversation list endpoints are cursor-paginated and remain stable under live updates
- The API is stateless from the frontend's perspective; state lives in Rust-owned storage and runtime
- CORS allows only the configured frontend origin(s)
