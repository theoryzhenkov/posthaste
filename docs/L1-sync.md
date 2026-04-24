---
scope: L1
summary: "Sync loop, state tokens, sync batch writes, mailbox reconciliation, event log"
modified: 2026-04-24
reviewed: 2026-04-24
depends:
  - path: docs/L0-sync
  - path: docs/L1-jmap
  - path: docs/L0-api
dependents:
  - path: docs/L1-search
  - path: docs/L1-compose
  - path: docs/L1-ui
  - path: docs/L2-transport
---

# Sync domain -- L1

## Sync loop

The sync engine runs as a Rust async task per enabled account. Three triggers cause a sync cycle:

- startup
- remote push notification
- periodic poll or manual sync

For each cycle, the engine loads stored cursors from SQLite, then syncs mailbox state and email state. There is no standalone thread delta fetch for the local conversation list; thread and conversation projections are derived from synced Email records, using JMAP `threadId` as the authoritative grouping key for JMAP accounts.

## State management

State strings are per-type, per-account, and stored in `sync_cursor`. The engine reads them on startup and after every successful cycle.

If the server returns `cannotCalculateChanges`, the engine falls back:

- mailbox sync falls back from `Mailbox/changes` to `Mailbox/query + Mailbox/get`
- email sync falls back from `Email/changes` to `Email/query + Email/get`

This fallback is part of the normal sync contract and must preserve correctness even after long offline periods. RFC 8620 treats `cannotCalculateChanges` as cache invalidation for the affected object type. A full fallback snapshot is authoritative: local objects of that type that are absent from the snapshot are stale and must be removed before the new cursor is persisted.

## SyncBatch and apply_sync_batch

The primary write path is `apply_sync_batch`, which receives a single `SyncBatch` and executes it within one SQLite transaction. Bodies fetched on demand are still stored separately because they happen outside the metadata sync loop.

Required `SyncBatch` shape:

```rust
pub struct SyncBatch {
    pub mailboxes: Vec<MailboxRecord>,
    pub messages: Vec<MessageRecord>,
    pub deleted_mailbox_ids: Vec<MailboxId>,
    pub deleted_message_ids: Vec<MessageId>,
    pub replace_all_mailboxes: bool,
    pub replace_all_messages: bool,
    pub cursors: Vec<SyncCursor>,
}
```

The important details are `replace_all_mailboxes` and `replace_all_messages`:

- `false` for delta mailbox syncs from `Mailbox/changes`
- `true` for full mailbox snapshots from `Mailbox/query + Mailbox/get`
- `false` for delta email syncs from `Email/changes`
- `true` for full email snapshots from `Email/query + Email/get`

When `replace_all_mailboxes` is true, the store compares the current local mailbox IDs for the account to the mailbox IDs present in the incoming snapshot and deletes any stale local rows before applying upserts. This is what prevents removed server mailboxes from persisting forever in the sidebar after a full resync.

When `replace_all_messages` is true, the store performs the same reconciliation for messages. This prevents deleted or expunged remote email from surviving locally after an `Email/changes` cursor gets too old for the server to calculate.

## SQLite schema

The runtime schema is centered around raw message state plus locally derived projections:

- `mailbox`
- `message`
- `conversation`
- `conversation_message`
- `message_mailbox`
- `message_keyword`
- `message_body`
- `thread_view`
- `sync_cursor`
- `event_log`
- `source_projection`

Important derived tables:

- `conversation` stores the latest message pointer, display subject, unread count, and message count used by the paginated middle pane. For JMAP accounts, conversation identity is derived from server `threadId`, not from subject or RFC 5322 threading headers.
- `conversation_message` links conversation IDs to concrete `(account_id, message_id)` pairs.
- `event_log` stores ordered domain events with a monotonically increasing `seq`, which powers `/v1/events`.
- `sync_cursor` stores per-account mailbox and message state strings.

## Conversation pagination

Conversation pages are generated inside the store from the `conversation` and `message` projections using seek pagination:

- sort key: `latest_received_at DESC, conversation_id DESC`
- cursor contents: `latest_received_at` plus `conversation_id`
- query strategy: "strictly older than this cursor", never `OFFSET`

This matters because the frontend keeps many pages cached while live updates keep arriving at the top. Seek pagination is stable under prepend-heavy workloads; offset pagination is not.

## Event propagation

`apply_sync_batch` emits domain events as it mutates the store:

- mailbox upserts and deletions emit `mailbox.updated`
- message deletions emit `message.updated`
- mailbox membership changes can emit `message.arrived` and `message.mailboxes_changed`

These events are inserted into `event_log` and also published over the local broadcast channel used by `/v1/events`. The frontend consumes that ordered stream and decides whether to invalidate or merge.

## Conflict model

Mutations include optimistic concurrency checks when the gateway supports them. If the server returns `stateMismatch`, the engine re-syncs the affected type and presents the updated state to the UI. The original mutation is not retried blindly.

## Error handling

The important sync failure mode is `cannotCalculateChanges`. That is not treated as a terminal error; it is converted into a full resync for the affected object type. Database failures still abort the transaction and surface as sync failures.

## Invariants

- The local SQLite database is the single source of truth for the UI. The frontend reads via the REST API, never directly from JMAP.
- State strings are persisted atomically with data in a single SQLite transaction.
- A failed sync never leaves the database inconsistent. `apply_sync_batch` runs in a single transaction.
- Full mailbox snapshots are authoritative: missing remote mailboxes are pruned locally when `replace_all_mailboxes` is true.
- Full email snapshots are authoritative: missing remote messages are pruned locally when `replace_all_messages` is true.
- Conversation projections are derived from synced message state and use server `threadId` as the grouping key for JMAP mail.
- Email bodies are fetched lazily on first view, not during metadata sync.

## Assertions

| ID | Sev. | Assertion |
|----|------|-----------|
| cache-source-of-truth | MUST | Frontend reads only via REST API, never directly from JMAP |
| state-atomic | MUST | State strings persisted in same SQLite transaction as data via apply_sync_batch |
| snapshot-authoritative | MUST | Full mailbox snapshots prune stale local mailboxes when they disappear remotely |
| message-snapshot-authoritative | MUST | Full email snapshots prune stale local messages when they disappear remotely |
| body-lazy | MUST | Email bodies are fetched on first view, not during metadata sync |
| fallback-resync | MUST | On cannotCalculateChanges, engine performs full resync for the affected type |
| conversation-derived | MUST | Conversation summaries are derived from local message projections using JMAP threadId for JMAP sources |
| event-log-ordered | MUST | Local domain events are ordered by `event_log.seq` and replayable via `afterSeq` |
| transaction-scope | MUST | apply_sync_batch executes within a single SQLite transaction |
