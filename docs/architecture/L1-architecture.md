---
title: "Architecture (L1)"
scope: L1
summary: "The overview of the architecture of Posthaste. TL;DR: Posthaste is a single integrated desktop app — a TS React frontend and a Rust backend wrapped in a Tauri application. The backend owns the SQLite store and evaluates every query; the frontend renders live query results."
modified: 2026-07-17
reviewed: 2026-07-17
state: active
depends:
dependents:
---

# Architecture

Posthaste is a single integrated desktop application. A Rust backend owns all
mail state and is its only evaluator: it syncs providers, folds pending local
changes, and answers every query. A TypeScript React frontend declares
queries and renders their live results. The two halves speak a small
localhost protocol — queries and commands up, an event stream down — and ship
together in a Tauri shell.

## Frontend

A React app running in the Tauri webview — or, identically, in a plain
browser tab against a local backend. Plain, dependency-light TypeScript,
intended to be read and modified in place.

### Data access

Components read through a client facade provided once via app context:

```tsx
const mail = useMailClient()

const inbox  = useMailList({ in: 'inbox' })          // live
const counts = useMailboxCounts()                    // live
const hits   = await mail.query({ text: 'invoice' }) // one-shot
```

A live hook declares a query; the facade fetches it, renders the answer, and
refetches when the store changes. Global state is the same shape: counters,
sync status, and pending operations are queries with a wider scope. This is
the Room / SwiftData paradigm — declared queries that re-run when the
database changes — with the invalidation ping crossing a socket instead of a
function call.

### Liveness

The backend broadcasts one SSE event stream. Every message carries the
monotonic store generation — bumped on every write, repeated as a
heartbeat — and most also carry a domain event the frontend uses as a
prompt (a desktop notification, a focused refetch). When the stream passes
a mounted query's generation, the facade refetches it on a short debounce;
answers carry their generation, so late responses are discarded. The
mechanism is level-triggered: a missed message heals at the next one, and
reconnecting is the same code path as connecting.

### Commands

User actions call facade verbs — `mail.archive(id)`, `mail.send(draft)` —
which post typed commands. The backend applies the effect to its store and
returns the new generation; the facade refetches up to it, so the user sees
the result in the next frame. A command's progress and verdict are themselves
queryable state (the pending-operations query).

## Backend

A Rust process, embedded in the Tauri app and runnable standalone for dev and
headless use.

### Provider sync

JMAP and IMAP/SMTP engines replicate each account's mail into the local store
and push outbound changes. The store is the device's replica; the provider
remains the ultimate authority.

### The store

One SQLite database holds base mail state (provider truth, written only by
sync), the overlay of not-yet-confirmed local changes, and the outbox of
pending operations. Reads go through effective views that serve base +
overlay already folded, so every consumer sees one coherent, optimistic
state.

### Queries

The read surface: windowed domain queries — lists, threads, counts, search,
pending operations — evaluated in SQL over the effective views. Every write
bumps the store generation and emits it on the event stream.

### Commands

Typed intents. Mail mutations enter the outbox, fold into the overlay in the
same transaction — that fold is the optimism the user sees — and settle
against the provider asynchronously; their verdicts are queryable. The
backend serves HTTP + SSE on localhost, and the same endpoints serve the
frontend, the CLI, the MCP server, and user scripts: one integration surface.

## Model

The state model is a single derivation pipeline; every stage has exactly one
writer:

```
provider ──sync──▶ base ────┐
                            ├──▶ effective ──queries──▶ screen
commands ─▶ outbox ─▶ overlay
                            └──── every write ─▶ event stream ─▶ refetch
```

Base is provider truth, written only by sync. The overlay is the folded
effect of pending intents and the only home of optimism; when the provider
confirms an intent, sync writes the truth into base and the overlay entry
retires. Effective state is what every read sees — the frontend is unaware
optimism exists. Commands are the only way anything moves up, and every
caller — UI, CLI, agent — submits the same intents and observes the same
state.

Consistency follows from the shape: one evaluator, global invalidation, and
queries as the only read. A lost stream message costs bounded staleness and heals at
the next heartbeat.
