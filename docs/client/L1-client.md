---
title: "Client (L1)"
scope: L1
summary: "The frontend contract: one facade over the API — live queries that refetch on the event stream, commands as facade verbs, ephemeral state in components, event payloads as prompts."
modified: 2026-07-17
reviewed: 2026-07-17
state: draft
depends:
  - path: docs/architecture/L1-architecture
  - path: docs/api/L1-api
dependents:
---

# Client

## 1. Boundary

The client is a React app in the Tauri webview, and identically a page in
any browser pointed at a local backend. It speaks only the API — queries,
commands, the event stream, blobs — and holds no mail logic: every list it
shows is a query answer, every action it takes is a command. The code is
plain TypeScript with few dependencies, written to be read and modified in
place.

## 2. The facade

One `MailClient` object, created at startup from the backend's
connection-info and provided through app context, is the only way components
touch the API. It exposes live query hooks, one-shot reads, and command
verbs; it owns the event-stream subscription, the refetch loop, and the
generated wire types. Components never see HTTP, SSE, or generations.

## 3. Live queries

A hook declares a query and renders its latest answer:

```tsx
const inbox = useMailList({ in: 'inbox' })   // { data, generation, status }
```

Mounting fetches; the answer is held with its generation. When the event
stream reports a newer generation, the facade refetches every mounted query
on a short debounce; answers arriving out of order are discarded by their
generation stamp. Identical queries from different components share one
watch. Unmounting forgets the query — there is no cache beyond what is
mounted, and nothing is ever served from memory that the backend has not
just said.

Connection state is part of the contract: while the backend is unreachable
the facade reports it, keeps the last answers marked stale for display, and
on reconnect refetches everything mounted — recovery is the connect path.

## 4. Commands

Actions are facade verbs that post typed commands with generated ids:

```tsx
mail.archive(messageId)
mail.send(draft)
```

The verb resolves when the backend accepts, and the facade immediately
refetches up to the returned generation — the archived row disappears on the
next frame because the backend's answer changed, never because the client
edited a list. Provider outcomes arrive later as pending-operations state,
which the UI renders like any other query (an outbox pane, an undo toast).

## 5. Ephemeral state

Text being composed, selection, focus, open panes, and in-flight form input
are ordinary component state. They become real on submission, as commands;
until then the backend does not know they exist. Nothing mail-shaped lives
in component state — the moment data describes mail rather than editing, it
is a query answer.

## 6. Prompts

Event payloads drive reactions, not state: a `message.arrived` event may
raise a desktop notification or focus a refetch; an `operation.settled`
event may prompt the undo toast to resolve. Every reaction ends in a query
or a command — payloads are never written into anything the UI renders
from.

## 7. Hackability

A user modification and a first-party feature are the same kind of code:
import the facade, declare queries, call verbs. The generated types make the
wire contract discoverable from the editor; the facade makes liveness
automatic; capability tokens (minted in the UI) let an integration run
outside the app with the same API. The contract for extensions is the same
two sentences as for the app itself: read state through queries, change it
through commands.
