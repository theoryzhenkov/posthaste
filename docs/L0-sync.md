---
scope: L0
summary: "Why local replica, sync model decisions, online-first strategy"
modified: 2026-04-01
reviewed: 2026-04-23
depends:
  - path: README
  - path: docs/L0-jmap
  - path: docs/L0-api
dependents:
  - path: docs/L1-sync
  - path: docs/L0-search
  - path: docs/L0-compose
  - path: docs/L0-accounts
---

# Sync domain -- L0

## Why a local replica

The UI reads from a local SQLite database, not from the network. Every sidebar view, conversation list, message detail lookup, and search query hits the local cache through the REST API. This gives fast rendering regardless of network conditions and lets users read previously synced mail offline.

The local store is a cache, not a source of truth. The JMAP server is authoritative. If the local database is corrupted or deleted, a full resync restores it from scratch. No user data lives exclusively on the client.

## JMAP's sync model

JMAP provides delta sync through `*/changes` and related query endpoints. Each response includes a state string. The client persists that state and passes it back on the next request to receive only what changed since that state.

For providers that support push, the sync engine consumes the remote push stream and turns resulting changes into local domain events. The local web API then exposes those domain events to the frontend via EventSource. There are therefore two push layers:

- remote JMAP push into the Rust sync engine
- local SSE from the Rust daemon into the browser

A periodic fallback poll still exists to catch missed events and providers that cannot maintain a stable push stream.

## Online-first for v1

The MVP requires network connectivity for all mutations: move, delete, flag, send. Mutations go directly to the server via JMAP with optimistic concurrency checks. There is no offline mutation queue in v1. Offline reading of already-synced mail still works because the cache is populated locally.

## Rust-owned SQLite

Rust owns the SQLite database directly via `rusqlite`. There is no dual-language cache ownership and no callback interface. The sync engine writes to SQLite in the same process, and the web API reads from it to serve the frontend.

The tradeoff is that the frontend does not get direct database notifications. It relies on the API's event stream and explicit HTTP refetches to observe changed state.

## Sync granularity

Mailbox metadata and email metadata (headers, preview, flags, mailbox membership, keywords) are fully synced. Conversation projections are derived locally from message records. These metadata sets are small enough that even large mailboxes remain practical to sync incrementally.

Email bodies and attachments are fetched lazily on first view. A typical email body is 10-100KB; syncing 100k bodies upfront would take hours and waste bandwidth for mail the user may never open. The body is cached in SQLite after the first fetch, so subsequent views are instant.

## Full snapshot reconciliation

Incremental mailbox syncs can carry explicit deletions. Full mailbox syncs cannot assume that an omitted mailbox should remain locally. The full-sync result is treated as an authoritative snapshot, so mailboxes missing from that snapshot are removed from the local store during `apply_sync_batch`.

This matters because providers can force the client to fall back from delta sync to full sync. Without authoritative pruning, deleted remote mailboxes can survive indefinitely in the local sidebar even though they no longer exist on the server.

## Risk

The main risk is SQLite write contention during heavy sync while the API is serving reads. WAL mode mitigates this by allowing concurrent reads during writes.
