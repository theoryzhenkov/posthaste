---
title: "API (L1)"
scope: L1
summary: "The one integration surface: localhost HTTP + SSE serving queries, commands, the event stream, and binary resources — the same contract for the frontend, the CLI, the MCP server, and user scripts."
modified: 2026-07-17
reviewed: 2026-07-17
state: draft
depends:
  - path: docs/architecture/L1-architecture
dependents:
---

# API

## 1. Boundary

The backend serves one API on a loopback port and writes its connection info
(port + session secret) to the app data directory, where the frontend, the
CLI, and scripts discover it. Every consumer speaks the same contract: JSON
in camelCase over HTTP, plus one SSE event stream.
The request and response types are defined once, in the Rust models crate,
and the TypeScript types are generated from them — the wire shapes have one
source of truth.

## 2. Queries

All reads go through `POST /query`. The body is one typed query — a mail
list, a thread, a message detail, mailbox counts, accounts, pending
operations, search — and the response is its result together with the store
generation it was computed at:

```json
{ "generation": 4182, "data": { "rows": [...] } }
```

Queries are windowed: list queries take a limit and an opaque cursor, and
return a screenful, never the mailbox. Evaluation happens entirely in the
backend, over the effective views, so every answer already includes the
folded effect of pending local operations. Queries are side-effect-free and
always answered fresh — the API has no read cache, and needs none on
loopback.

## 3. The event stream

`GET /events` is one SSE broadcast serving both liveness and automation.
Every message carries the current store generation; most also carry a domain
event — message arrived, operation settled, sync completed — and a
generation-only heartbeat fills silences:

```json
{ "generation": 4183, "event": { "kind": "message.arrived", "accountId": "a1", "messageId": "m9" } }
{ "generation": 4184 }
```

The two dimensions have different guarantees. The generation is
level-triggered and loss-proof: every message states current state, so a
dropped message heals at the next one, and reconnecting is the same code
path as connecting. Generations are monotonic within one backend run; a
fresh run id tells clients to treat everything they hold as stale. The
backend keeps no per-client state — the stream is one broadcast.

Event payloads are prompts, never a ledger: a lagging consumer loses
payloads but keeps liveness, and anything that needs completeness
reconciles through queries. Payloads trigger reads — a refetch, a desktop
notification, a `watch --exec` script, a webhook rule — and are never
folded into client-held state; state comes from queries alone. A client
that only wants liveness reads only the generation and refetches the
queries that are behind.

## 4. Commands

All writes go through `POST /command`. The body is one typed intent — the
same vocabulary the outbox stores — with a client-generated idempotency id:

```json
{ "id": "01J...", "command": { "archive": { "messageId": "..." } } }
```

The backend applies the command's local effect (for mail mutations: enqueue
in the outbox and fold into the overlay, in one transaction) and replies
with the resulting generation. An answer at or past that generation shows
the effect, which is how a caller reads its own writes. Retrying the same id
is safe; the command applies once.

Provider settlement is asynchronous. A command's acceptance means "recorded
and visible", and its eventual verdict — delivered, rejected, parked — is
state, observed through the pending-operations query like any other data.

## 5. Binary resources

Message bodies, attachments, and other blobs are fetched by id over plain
authenticated GETs. Blobs are immutable, so responses carry long-lived
caching headers. Compose uploads go the other way: `POST /blobs` stores an
attachment and returns the blob id that the send command references.

## 6. Authentication

Every request bears the session secret from the connection info file — the
possession of which is the local trust boundary, equivalent to local user
access. Agents and scripts get their own credentials: scoped, auto-expiring
tokens minted through the UI or CLI, carrying an explicit capability set
(which queries, which commands, which accounts). The MCP server holds such a
token, never the session secret.

## 7. Errors

Failures use one envelope: a typed kind, a human-readable message, and a
retryability flag. Malformed requests, unknown ids, and capability misses
fail the HTTP call. A provider failure does not: the command was accepted
and recorded, so the failure arrives later as the operation's verdict —
queryable state, not an HTTP error. The error kinds are part of the
generated models, so clients match on types rather than strings.
