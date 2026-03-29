---
scope: L0
summary: "Why local replica, sync model decisions, online-first strategy"
modified: 2026-03-29
reviewed: 2026-03-29
depends:
  - path: README
  - path: spec/L0-jmap
  - path: spec/L0-bridge
dependents:
  - path: spec/L1-sync
  - path: spec/L0-search
  - path: spec/L0-compose
  - path: spec/L0-accounts
---

# Sync domain -- L0

## Why a local replica

The UI reads from a local SQLite database, not from the network. Every list view, thread view, and search query hits GRDB on disk. This gives instant rendering regardless of network conditions and lets users read previously synced mail offline.

The local store is a cache, not a source of truth. The JMAP server is authoritative. If the local database is corrupted or deleted, a full resync restores it from scratch. No user data lives exclusively on the client.

## JMAP's sync model

JMAP provides delta sync through `*/changes` and `*/queryChanges` endpoints. Each response includes a state string. The client persists this string and passes it back on the next request to receive only what changed since that state: creates, updates, and destroys. This replaces IMAP's poll-and-diff approach entirely.

For Fastmail, push notifications arrive via EventSource. The server sends a state change event whenever any mail object changes. The client reacts by running a sync cycle, so there is no polling interval in normal operation. A periodic fallback poll (every 60s) catches missed events.

## Online-first for v1

The MVP requires network connectivity for all mutations: move, delete, flag, send. Mutations go directly to the server via `Email/set` with `ifInState` for optimistic concurrency control. If the server's state has moved since the client last synced, the server returns `stateMismatch` and the client re-syncs before retrying.

There is no offline mutation queue in v1. Adding one requires conflict resolution, retry ordering, and merge logic that adds significant complexity for a feature most users won't need on a desktop client with a stable connection. Offline reading of synced mail works because the cache is already populated.

## GRDB as the cache

Swift's GRDB library wraps SQLite and provides `ValueObservation` for reactive queries. When a row changes, GRDB notifies any active observer, and SwiftUI views update automatically. Rust writes to GRDB via the `CacheWriter` callback interface defined in the bridge layer.

The alternative was Rust-owned SQLite with manual change notifications pushed to Swift. GRDB was chosen because its observation API eliminates the notification plumbing between Rust writes and SwiftUI updates. The tradeoff is that Swift owns the database schema and migration lifecycle.

## Sync granularity

Mailbox metadata, Thread metadata, and Email metadata (headers, preview, flags) are fully synced. These are small enough that even a mailbox with 100k messages syncs metadata in seconds.

Email bodies and attachments are fetched lazily on first view via `Blob/download`. A typical email body is 10-100KB; syncing 100k bodies upfront would take hours and waste bandwidth for mail the user may never open. The body is cached in GRDB after the first fetch, so subsequent views are instant.

## Risk

The main risk is GRDB write performance under heavy sync load during initial sync of a large account. Tens of thousands of email metadata records need to be inserted in a short window. This is mitigated by batched writes within GRDB transactions via `CacheWriter`, keeping each transaction under a few thousand rows.
