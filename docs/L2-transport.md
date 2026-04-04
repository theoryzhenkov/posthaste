---
scope: L2
summary: "JMAP transport abstraction: WS preferred with SSE fallback, JmapTransport for API routing, resilient push streams"
modified: 2026-04-02
reviewed: 2026-04-02
depends:
  - path: docs/L1-jmap
  - path: docs/L1-sync
  - path: docs/L0-sync
dependents: []
---

# Transport layer -- L2

## Problem

The client currently hardcodes SSE (EventSource) for push notifications and HTTP POST for every JMAP API call. JMAP also defines a WebSocket transport (RFC 8887) that carries both push notifications and request/response pairs on a single persistent connection. WebSocket reduces connection overhead and latency, particularly for mutation-heavy workflows where each flag toggle or move currently opens a new HTTP request.

The client should prefer WebSocket when the server supports it, fall back to SSE for push when it doesn't, and always have HTTP as a last resort for API calls.

## Transport negotiation

On connection, the gateway reads the JMAP Session object. If the session advertises `urn:ietf:params:jmap:websocket` in its capabilities map, the gateway opens a WebSocket connection and routes both API calls and push notifications through it. If the capability is absent, the gateway falls back to SSE for push and HTTP for API calls. This is automatic and silent -- no user-facing configuration. All accounts use the same strategy: prefer WS, fall back to SSE.

## New abstractions

Three new traits replace the current monolithic approach.

### JmapTransport

Handles request/response for JMAP method calls. Two implementations:

- **HttpTransport** -- current behavior. Each `send` creates an HTTP POST to the JMAP API URL. Stateless.
- **WsTransport** -- sends requests as JSON frames over a persistent WebSocket. Receives responses (and push notifications) on the same stream. Requires request/response correlation.

The `MailGateway` trait stays unchanged externally. `LiveGateway` holds a `JmapTransport` internally and routes calls through it. When the WS connection drops, `LiveGateway` transparently falls back to `HttpTransport` for API calls and emits an event so the supervisor knows to reconnect.

### PushTransport

Opens a raw push notification stream. Two implementations:

- **SsePushTransport** -- wraps `jmap-client`'s `event_source()`. Returns a `PushStream`.
- **WsPushTransport** -- filters the WS message stream for push notifications only. Shares the underlying WS connection with `WsTransport`.

`PushTransport` is stateless and testable. It opens one connection and returns a stream. It does not reconnect.

### ResilientPushStream

A wrapper that consumes `PushTransport` implementations and adds:

- Reconnection with backoff on stream errors
- Automatic WS → SSE fallback when WS fails repeatedly
- A single `PushStream` output that the supervisor consumes without knowing which transport is active

The supervisor's current reconnection logic (retry timers, push status tracking) moves into `ResilientPushStream`. The supervisor becomes a consumer of one stream, not a manager of transport lifecycle.

## WebSocket connection lifecycle

One WebSocket connection per account. The connection is opened during the first sync trigger (same as the current gateway initialization). The WS stream yields an interleaved sequence of `WebSocketMessage::Response` and `WebSocketMessage::PushNotification`. A demultiplexer splits these:

- Responses are routed to the caller that sent the corresponding request (matched by `requestId`).
- Push notifications are forwarded to the `PushStream`.

Push is enabled on the WS connection immediately after open via `WebSocketPushEnable` with the relevant data types.

## Request/response correlation

When a caller sends a JMAP request over WS, the transport assigns a monotonically increasing `requestId`, sends the frame, and returns a future that resolves when the matching response arrives. A background task reads from the WS stream, matches responses to pending requests by ID, and forwards push notifications to the push stream.

This correlation layer lives in the `WsTransport` implementation, not in `MailGateway` or the supervisor.

**External fork work**: the `jmap-client` crate (stalwartlabs/jmap-client) provides low-level WS framing via `connect_ws()` and `send_ws()` but does not correlate requests to responses internally. The plan is to fork `jmap-client`, add a correlation layer there (oneshot channels keyed by request ID), and submit the change upstream. PostHaste uses the fork until the PR is accepted. If the upstream rejects the change, the fork continues independently.

## HTTP fallback

When the WS connection drops (network error, server restart, TLS renegotiation), the transport:

1. Switches API calls to `HttpTransport` immediately. In-flight WS requests fail; callers retry through HTTP.
2. Emits an event (`push.transport_fallback`) so the supervisor and UI can reflect the degraded state.
3. Attempts to reopen the WS connection with exponential backoff.
4. On successful WS reconnect, routes new API calls through WS again and re-enables WS push.

Push falls back from WS to SSE through `ResilientPushStream`, which tries WS first, then SSE, with independent backoff timers for each.

## What doesn't change

- `MailGateway` trait methods stay the same. Callers don't know about transport details.
- `MailService` is unaffected. It calls gateway methods as before.
- The supervisor still triggers syncs on push notifications. It just gets them from `ResilientPushStream` instead of managing the raw push stream directly.
- The mock gateway in tests continues to work without any transport abstraction.

## Phasing

This is a two-phase implementation:

**Phase 1**: Extract `PushTransport` trait and `ResilientPushStream`. Add `WsPushTransport` alongside existing SSE. Supervisor consumes the resilient stream. API calls stay HTTP.

**Phase 2**: Add `JmapTransport` trait. Implement `WsTransport` with request correlation. Route API calls through WS with HTTP fallback. This requires the `jmap-client` fork.

## Assertions

| ID | Sev. | Assertion |
|----|------|-----------|
| ws-preferred | MUST | WebSocket is attempted before SSE when the server advertises the capability |
| sse-fallback | MUST | If WebSocket is unavailable or fails, push falls back to SSE without user intervention |
| http-fallback | MUST | If the WebSocket connection drops, API calls transparently fall back to HTTP |
| gateway-unchanged | MUST | MailGateway trait methods do not change; transport is an internal concern of the gateway |
| correlation-by-id | MUST | WebSocket responses are matched to requests by requestId, not by ordering |
| resilient-stream | MUST | ResilientPushStream reconnects automatically and falls back WS→SSE without supervisor involvement |
| single-ws-per-account | SHOULD | Each account maintains at most one WebSocket connection for both API and push |
