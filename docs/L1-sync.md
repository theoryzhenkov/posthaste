---
scope: L1
summary: "Sync loop, state tokens, sync batch writes, mailbox reconciliation, event log"
modified: 2026-04-27
reviewed: 2026-04-27
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

The runtime emits INFO-level structured progress logs for sync start, provider discovery, mailbox/message fetch phases, store writes, and sync completion/failure. Each sync cycle has a `sync_id` span field so nested gateway and store events can be queried as one operation. IMAP sync logs per-mailbox planning decisions, per-mailbox header fetch start/completion, and chunked header fetch progress; JMAP full snapshots log ID discovery and metadata chunk progress. These diagnostics are intentionally backend logs first; user-facing progress UI consumes a smaller account progress model rather than raw log lines.

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
    pub imap_mailbox_states: Vec<ImapMailboxSyncState>,
    pub imap_message_locations: Vec<ImapMessageLocation>,
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

IMAP sync batches also carry `imap_mailbox_states` and
`imap_message_locations`. These rows are applied in the same transaction as
their `MessageRecord`s so later sync cycles, lazy body fetches, and mutations
can use persisted IMAP command state without deriving it from presentation
fields.

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
- `automation_backfill_job`
- `sender_address_cache`

Important derived tables:

- `conversation` stores the latest message pointer, display subject, unread count, and message count used by the paginated middle pane. For JMAP accounts, conversation identity is derived from server `threadId`, not from subject or RFC 5322 threading headers.
- `conversation_message` links conversation IDs to concrete `(account_id, message_id)` pairs.
- `event_log` stores ordered domain events with a monotonically increasing `seq`, which powers `/v1/events`.
- `sync_cursor` stores per-account mailbox and message state strings.
- `imap_mailbox_sync_state` stores IMAP cursor state per account and mailbox:
  mailbox ID/name, `UIDVALIDITY`, highest UID, and `HIGHESTMODSEQ` when
  available. This table is separate from JMAP-style `sync_cursor` because IMAP
  validity and delta state are mailbox-scoped.
- `imap_message_location` stores the mailbox UID locations that make an IMAP
  message addressable for fetch and mutation commands. This is separate from
  message identity so provider-stable IDs such as Gmail `X-GM-MSGID` can
  deduplicate messages across labels while retaining per-mailbox UIDs.
- `automation_backfill_job` stores durable per-account work for the current automation-rule fingerprint, so completed backfills are not repeated after restart while changed rules enqueue fresh work.
- `sender_address_cache` stores account-scoped sender addresses that previously passed provider submission. Entries are keyed by `(account_id, normalized_email)`, ordered by `last_used_at`, and used only as compose suggestions.

The store maintains account-scoped indexes for message-page reads, including received date and the sortable sender, subject, flagged, and attachment keys used by the message list. These indexes support seek pagination without making the frontend maintain a duplicate message index.

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

## Automation actions

After a sync batch is written, global automation rules evaluate matching synced message records from that batch. Rules have explicit triggers, smart-mailbox-style conditions, and typed actions. Account and mailbox targeting is expressed as ordinary conditions. The backend still evaluates each rule inside the current account runtime and adds the current `source_id` plus the synced message IDs to the internal query before applying actions through that account's gateway. The initial settings UI creates an account-conditioned rule equivalent to:

- if a message belongs to account `A` and its sender display name or email contains text `X`, apply user tag `Y`

Actions mutate the remote server through the same JMAP command paths as manual message actions, then persist the returned mutation locally. Supported action variants include applying/removing user tags, read/unread, flag/unflag, and moving a message to a target mailbox. Action execution is idempotent: if the target state is already true, the action is skipped. Action mutations happen after `apply_sync_batch`, so they do not weaken the atomicity of the incoming metadata write.

The account runtime also performs automatic backfill for existing local messages. Backfill is intentionally low priority: it runs only while the account runtime has a live gateway, starts after a delay, processes a small bounded batch, publishes resulting mutation events, then waits before the next batch. Foreground sync, push handling, and manual commands remain the primary work of the runtime.

Backfill scheduling is backend-owned and durable. The domain service fingerprints the enabled `backfill = true` automation rules, creates a pending `automation_backfill_job` for each enabled account when rules change or when an account runtime first observes missing current work, and marks the job completed once a batch reports no more matching work. Worker failures increment the job attempt count and keep it pending for a later retry.

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
| imap-state-per-mailbox | MUST | IMAP sync state is stored per account and mailbox, including UIDVALIDITY and optional MODSEQ |
| imap-locations | MUST | IMAP message command locations are stored separately from local message identity |
| conversation-derived | MUST | Conversation summaries are derived from local message projections using JMAP threadId for JMAP sources |
| event-log-ordered | MUST | Local domain events are ordered by `event_log.seq` and replayable via `afterSeq` |
| transaction-scope | MUST | apply_sync_batch executes within a single SQLite transaction |
| automation-backfill-durable | MUST | Automation backfill progress is stored in SQLite and completed jobs for the same rule fingerprint do not rerun after restart |
