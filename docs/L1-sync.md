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

The sync engine runs as a Rust async task per enabled account. Four triggers cause a sync cycle:

- startup
- remote push notification
- periodic poll
- manual sync

The poll timer is scheduled after a sync cycle completes and uses skipped
missed-tick behavior. Startup, manual, and push-triggered syncs therefore reset
the next poll deadline instead of allowing an overdue periodic tick to run
immediately after a long cycle.

For each cycle, the engine loads stored cursors from SQLite, then syncs mailbox state and email state. There is no standalone thread delta fetch for the local conversation list; thread and conversation projections are derived from synced Email records, using JMAP `threadId` as the authoritative grouping key for JMAP accounts.

The runtime emits INFO-level structured progress logs for sync start, provider discovery, mailbox/message fetch phases, store writes, and sync completion/failure. Each sync cycle has a `sync_id` span field so nested gateway and store events can be queried as one operation. IMAP sync logs per-mailbox planning decisions, conservative STATUS no-op skips, per-mailbox header fetch start/completion, and chunked header fetch progress; JMAP full snapshots log ID discovery and metadata chunk progress. These diagnostics are intentionally backend logs first; user-facing progress UI consumes a smaller account progress model rather than raw log lines.

Local cache diagnostics are structured backend logs as well. Candidate
generation logs the account, driver, fetch unit, candidate counts, scored byte
totals, and per-candidate score details at TRACE. Cache worker batches log
budget inputs, scan limits, candidate counts, admission decisions, fetch starts,
fetch failures, and successful stores at DEBUG. Supervisor logs summarize each
worker batch with scanned, attempted, cached, failed, and skipped counts.

While a sync is running, the supervisor stores compact user-facing progress in
`AccountRuntimeOverview.syncProgress`. Progress contains the sync ID, trigger,
start timestamp, stage (`connecting`, `discovering`, `planning`, `fetching`,
`storing`, or `waiting`), a short detail string, and optional mailbox/message
counts. Gateways report provider-specific phases through this stable model:
JMAP reports discovery and mailbox/message fetch phases, while IMAP reports
capability discovery, mailbox planning, per-mailbox no-op skips, and mailbox
fetch phases. The supervisor clears progress on success or failure.

## Local cache planning

Metadata remains mandatory and outside eviction. Optional local cache objects are
scored per layer:

- body
- raw message
- attachment blob

The first cache planner uses manual utility scoring. A candidate's priority is:

```text
priority = utility / size_cost
size_cost = (max(fetch_bytes, 4 KiB) / 1 MiB) ^ alpha
```

Default `alpha` is `0.7`, so large objects are penalized without making high
utility large attachments impossible to cache.

Message utility is a weighted sum of normalized signals:

```text
message_utility =
  0.35 * recency
+ 0.20 * thread_activity
+ 0.15 * sender_affinity
+ 0.10 * explicit_importance
+ 0.10 * search_context
+ 0.10 * local_behavior
```

Recency uses a 30-day half-life. Thread, sender, and local behavior signals are
saturating decayed counts. Explicit importance is derived from flags, unread
state, and Inbox membership. Search context is stronger for tight result sets
and top-ranked visible results:

```text
search_context =
  (1 - ln(result_count + 1) / ln(total_messages + 1))
  * (1 / sqrt(result_rank + 1))
```

Every message has exactly one structural body `cache_object` row, keyed by
`(account_id, message_id, layer = body, object_id = '')`. `message` remains the
source of truth for message existence and metadata; `cache_object` is the child
ledger for optional-content work and cache state. Sync writes create the body
row in the same transaction as message metadata, lazy body writes mark it
`cached`, message deletes remove cache child state, and store startup repairs
legacy databases that have messages without body cache rows.

The first implemented candidate source is baseline sync-time scoring. It uses
metadata available in the synced message record: recency, Inbox membership,
unread state, flagged state, and fetch size. Structural rows start with neutral
byte counts until the scorer materializes provider-aware values. Local user/app
activity then updates message-level signals separately from metadata sync.
Search result visibility is the first signal producer: visible ranked results
write search context and a rank-decayed direct user boost into
`cache_message_signal`, enqueue the message in `cache_rescore_queue`, and wake
the account runtime for cache maintenance. If a signal lands on a legacy message
that is missing its structural body row, the store materializes that row before
queueing the re-score. Opening/starred thread behavior and thread-level
activity should use the same signal queue instead of adding fetch-specific
shortcuts.

Layer weights are `1.0` for bodies, `0.45` for raw messages, and `0.25` for
attachment blobs. Attachment blobs receive object modifiers for inline
attachments and previously opened attachments.

`value_bytes` is the useful local content being prioritized. `fetch_bytes` is
the remote fetch/storage unit needed to satisfy the candidate and is the value
used by `size_cost`. JMAP body candidates can use a `body_only` fetch unit,
while IMAP body candidates use `raw_message` because IMAP cannot reliably fetch
the parsed body/attachment split that JMAP exposes. That makes large IMAP
messages naturally score lower for speculative background caching while still
allowing a high-utility message to win.

Cache budgets have a soft cap and a hard cap. Interactive work such as opening a
message or narrowing a search can raise the temporary target between those caps:

```text
effective_target =
  soft_cap + interactive_pressure * (hard_cap - soft_cap)
```

Admission is allowed when the candidate fits under `effective_target`. When it
does not, the candidate must beat the lowest-priority evictable cached object.
Admission is never allowed when it would cross the hard cap.

Metadata sync only records cache candidates; it does not fetch optional content.
The account runtime runs cache maintenance while a gateway is connected. Each
maintenance batch first queues a bounded oldest-first set of stale cache objects
whose `last_scored_at` is older than the runtime threshold, excluding objects
already queued or currently fetching. This lets time-sensitive signals such as
recency converge even when no sync/search/user signal touches a message.
The batch then consumes dirty re-score rows, rebuilds each candidate's current
signal set from message metadata plus `cache_message_signal`, updates
provider-aware `fetch_unit`, `value_bytes`, `fetch_bytes`, and
`cache_object.priority`, and marks non-cached/non-fetching objects `wanted`.
Unscored structural rows have `fetch_bytes = 0` and are not eligible for fetch
selection until this re-score step gives them a concrete fetch cost. The worker
then scans a bounded priority-ordered window of wanted body candidates, admits
only candidates that fit the current budget, marks them `fetching`,
fetches through the same gateway body path as lazy open, applies the body to the
local store, and marks the cache object `cached`. Periodic maintenance uses
background pressure; interactive maintenance triggered by visible search results
may use the burst space between the soft and hard caps. Scanning more rows than
the fetch-attempt cap prevents one large over-budget message from starving
smaller candidates behind it. Gateway failures mark the object `failed` with the
service error code and do not fail the whole runtime. Eviction and
attachment-blob workers are later policy layers, not part of the first worker
slice.

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
- `cache_object`
- `cache_message_signal`
- `cache_rescore_queue`

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
- `cache_object` stores scored optional-content child objects and their state (`wanted`, `fetching`, `cached`, `failed`, or `evicted`). Every message has one structural body row; later layers add raw-message or attachment rows as policy decides. Rows include `layer`, `fetch_unit`, `value_bytes`, `fetch_bytes`, priority, reason, timestamps, and last error code. Body cache workers read scored wanted rows from this table and cached rows contribute to the configured cache budget.
- `cache_message_signal` stores local cache utility signals that do not come from provider metadata, including search result visibility, local behavior scores, direct user boost, and pinned state.
- `cache_rescore_queue` stores account/message pairs whose cache objects need priority re-scoring after signal changes. The runtime drains this queue before selecting fetch work.

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

Account runtime transitions emit `account.status_changed`. Its payload includes
`status`, `push`, `lastSyncAt`, `lastSyncError`, `lastSyncErrorCode`, and
`syncProgress`, using the same camelCase enum values as REST responses. Progress
updates reuse this event so clients can update account settings and list rows
without polling logs.

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
- Metadata sync never fetches email bodies. Bodies are optional content fetched by lazy open or by the background cache worker after metadata is committed.

## Assertions

| ID | Sev. | Assertion |
|----|------|-----------|
| cache-source-of-truth | MUST | Frontend reads only via REST API, never directly from JMAP |
| state-atomic | MUST | State strings persisted in same SQLite transaction as data via apply_sync_batch |
| snapshot-authoritative | MUST | Full mailbox snapshots prune stale local mailboxes when they disappear remotely |
| message-snapshot-authoritative | MUST | Full email snapshots prune stale local messages when they disappear remotely |
| body-lazy | MUST | Metadata sync does not fetch email bodies; bodies are fetched by lazy open or cache maintenance after metadata is committed |
| fallback-resync | MUST | On cannotCalculateChanges, engine performs full resync for the affected type |
| cache-priority-size-aware | MUST | Optional cache priority uses fetch-unit bytes, so IMAP raw-message body fetches are penalized by combined message size |
| cache-worker-budget | MUST | Cache workers fetch only candidates admitted under the current cache budget and mark fetch failures in the cache ledger |
| cache-object-parity | MUST | Every synced message has one structural body `cache_object` child row, and message deletion removes its cache object, cache signals, and rescore queue rows |
| cache-signal-rescore | MUST | Local cache utility signals update `cache_message_signal`, enqueue `cache_rescore_queue`, and are applied by re-scoring before fetch selection |
| cache-stale-rescore | MUST | Cache maintenance periodically queues bounded oldest-first stale cache objects for re-scoring so recency and other time-sensitive utility signals converge |
| imap-state-per-mailbox | MUST | IMAP sync state is stored per account and mailbox, including UIDVALIDITY and optional MODSEQ |
| imap-locations | MUST | IMAP message command locations are stored separately from local message identity |
| conversation-derived | MUST | Conversation summaries are derived from local message projections using JMAP threadId for JMAP sources |
| event-log-ordered | MUST | Local domain events are ordered by `event_log.seq` and replayable via `afterSeq` |
| transaction-scope | MUST | apply_sync_batch executes within a single SQLite transaction |
| automation-backfill-durable | MUST | Automation backfill progress is stored in SQLite and completed jobs for the same rule fingerprint do not rerun after restart |
| sync-progress-runtime | SHOULD | Running account syncs expose compact user-facing progress and clear it on success or failure |
| cache-priority-size-aware | SHOULD | Optional body, raw-message, and attachment cache candidates are prioritized by manual utility divided by size cost |
| cache-admission-hard-cap | MUST | Optional cache admission may exceed the soft cap under pressure but must not exceed the hard cap |
