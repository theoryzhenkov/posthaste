---
scope: L1
summary: "Sync loop, state tokens, GRDB schema, CacheWriter interface, conflict model"
modified: 2026-03-29
reviewed: 2026-03-29
depends:
  - path: spec/L0-sync
  - path: spec/L1-jmap
  - path: spec/L0-bridge
dependents:
  - path: spec/L1-search
  - path: spec/L1-compose
  - path: spec/L1-ui
---

# Sync domain -- L1

## Sync loop

The sync engine runs as a Rust async task. Three triggers cause a sync cycle:

- EventSource push notification (primary path)
- Manual refresh from UI
- Periodic poll as fallback (every 60s)

On trigger, the engine calls `*/changes` for each type with the stored state string, processes the deltas, and calls `CacheWriter` methods to update GRDB.

A single sync cycle proceeds in this order:

1. `Mailbox/changes(sinceState)` -- apply mailbox creates, updates, destroys
2. `Email/changes(sinceState)` -- apply email creates, updates, destroys
3. `Thread/changes(sinceState)` -- apply thread updates

Steps 1-3 can be batched in a single JMAP request (multiple method calls in one HTTP POST). After all deltas are applied, the engine persists new state strings via `CacheWriter.writeState()`.

## State management

The Rust sync engine holds state strings in memory and persists them to GRDB via `CacheWriter`. State strings are per-type, per-account. On startup, the engine reads initial state from GRDB via `readState()` -- a one-time bootstrap read, not an ongoing dependency. After startup, Rust never reads from the cache.

If the server returns `cannotCalculateChanges` (state too old or server cannot compute the delta), the engine falls back to a full resync: `*/get` or `*/query` to rebuild the cache from scratch for the affected type. This is expected to be rare in normal operation but must be handled correctly because it will happen after extended offline periods.

## CacheWriter callback interface

Implemented by Swift, called by Rust via UniFFI.

The primary write method is `applySyncBatch`, which receives all changes from a sync cycle in a single call. The Swift implementation executes the entire batch within one GRDB transaction, guaranteeing that state strings and data are persisted atomically. This avoids partial-sync corruption if the process crashes mid-write.

`writeEmailBodies` is separate because body fetches happen outside the sync loop (on-demand when the user views an email). `readState` is called once at startup for bootstrap.

```
protocol CacheWriter {
    // Sync batch — all deltas from one sync cycle, applied in a single GRDB transaction
    func applySyncBatch(accountId: String, batch: FfiSyncBatch)

    // Body fetch — outside the sync loop, lazy on first view
    func writeEmailBodies(accountId: String, bodies: [FfiEmailBody])

    // Bootstrap — called once at startup
    func readState(accountId: String) -> FfiSyncState?
}
```

`FfiSyncBatch` groups all changes from a sync cycle:

```
FfiSyncBatch {
    newState: FfiSyncState                  # updated state strings
    mailboxesCreated: [FfiMailbox]
    mailboxesUpdated: [FfiMailbox]
    mailboxesDestroyed: [String]            # mailbox IDs
    emailsCreated: [FfiEmail]
    emailsUpdated: [FfiEmail]
    emailsDestroyed: [String]               # email IDs
    threadsCreated: [FfiThread]
    threadsUpdated: [FfiThread]
    threadsDestroyed: [String]              # thread IDs
}
```

## GRDB schema

All tables are account-scoped. Every primary key includes `account_id` as its first component.

```sql
CREATE TABLE mailbox (
    id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    name TEXT NOT NULL,
    parent_id TEXT,
    role TEXT,
    sort_order INTEGER NOT NULL DEFAULT 0,
    total_emails INTEGER NOT NULL DEFAULT 0,
    unread_emails INTEGER NOT NULL DEFAULT 0,
    total_threads INTEGER NOT NULL DEFAULT 0,
    unread_threads INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (account_id, id)
);

CREATE TABLE email (
    id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    thread_id TEXT NOT NULL,
    blob_id TEXT NOT NULL,
    received_at TEXT NOT NULL,  -- ISO 8601
    subject TEXT,
    from_name TEXT,
    from_email TEXT,
    preview TEXT,
    has_attachment INTEGER NOT NULL DEFAULT 0,
    size INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (account_id, id)
);

CREATE TABLE email_mailbox (
    account_id TEXT NOT NULL,
    email_id TEXT NOT NULL,
    mailbox_id TEXT NOT NULL,
    PRIMARY KEY (account_id, email_id, mailbox_id)
);

CREATE TABLE email_keyword (
    account_id TEXT NOT NULL,
    email_id TEXT NOT NULL,
    keyword TEXT NOT NULL,
    PRIMARY KEY (account_id, email_id, keyword)
);

CREATE TABLE email_recipient (
    account_id TEXT NOT NULL,
    email_id TEXT NOT NULL,
    type TEXT NOT NULL,  -- 'to', 'cc', 'bcc'
    name TEXT,
    email TEXT NOT NULL,
    PRIMARY KEY (account_id, email_id, type, email)
);

CREATE TABLE email_body (
    account_id TEXT NOT NULL,
    email_id TEXT NOT NULL,
    html TEXT,
    text_body TEXT,
    fetched_at TEXT,
    PRIMARY KEY (account_id, email_id)
);

CREATE TABLE thread (
    id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    email_ids TEXT NOT NULL,  -- JSON array of ordered email IDs
    PRIMARY KEY (account_id, id)
);

CREATE TABLE sync_state (
    account_id TEXT NOT NULL,
    type TEXT NOT NULL,  -- 'mailbox', 'email', 'thread'
    state TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (account_id, type)
);
```

### Indexes

- `email(account_id, thread_id)` for thread lookups
- `email(account_id, received_at)` for date sorting
- `email_mailbox(account_id, mailbox_id)` for mailbox listing
- `email_keyword(account_id, keyword)` for flag and tag queries

## FFI types

Flat records that cross the UniFFI boundary. No nested objects, no optionals where avoidable. Dates cross as `i64` unix timestamps; the Swift CacheWriter converts to ISO 8601 text for GRDB storage.

`FfiEmail`: id, threadId, blobId, receivedAt (i64 unix timestamp), subject (String?), fromName (String?), fromEmail (String?), preview (String?), hasAttachment (Bool), size (u64), mailboxIds ([String]), keywords ([String]), recipients ([FfiRecipient])

`FfiMailbox`: id, name, parentId (String?), role (String?), sortOrder (u32), totalEmails (u64), unreadEmails (u64), totalThreads (u64), unreadThreads (u64)

`FfiThread`: id, emailIds ([String])

`FfiSyncState`: mailboxState (String?), emailState (String?), threadState (String?)

`FfiRecipient`: type (String), name (String?), email (String)

`FfiEmailBody`: emailId (String), html (String?), textBody (String?), fetchedAt (i64)

`FfiSyncBatch`: newState (FfiSyncState), mailboxesCreated ([FfiMailbox]), mailboxesUpdated ([FfiMailbox]), mailboxesDestroyed ([String]), emailsCreated ([FfiEmail]), emailsUpdated ([FfiEmail]), emailsDestroyed ([String]), threadsCreated ([FfiThread]), threadsUpdated ([FfiThread]), threadsDestroyed ([String])

## Conflict model

All mutations include `ifInState` on `Email/set` and `Mailbox/set`. If the server returns `stateMismatch`, the engine re-syncs the affected type and presents the updated state to the UI. The original mutation is not retried automatically. The user sees the current state and can re-apply their action.

This is simple and correct for single-user mailboxes. Multi-user shared mailboxes would need a more sophisticated merge strategy, but that is out of scope.

## Error handling

```
SyncError
  |-- JmapError(JmapError)       -- protocol-level failure (wraps L1-jmap errors)
  |-- CacheWriteError(detail)    -- Swift GRDB write failed
  |-- StateTooOld                -- cannotCalculateChanges, need full resync
  |-- AccountNotFound(accountId) -- unknown account
```

All errors are typed and explicit. The sync engine does not silently retry or swallow failures. `StateTooOld` (server's `cannotCalculateChanges` response) triggers a full resync for the affected type. Note: this is distinct from `StateMismatch` in L1-jmap, which is the `ifInState` precondition failure on mutations. `CacheWriteError` is surfaced to the UI as a sync failure. `JmapError` is handled per the error model defined in L1-jmap.

## Invariants

- The local cache is the single source of truth for the UI. UI never reads from the network directly.
- State strings are persisted atomically with the data they describe, in a single `applySyncBatch` call.
- A failed sync never leaves the cache in an inconsistent state. `applySyncBatch` executes in one GRDB transaction.
- Sync is idempotent: replaying the same change deltas produces the same cache state.
- All tables include `account_id`. Multi-account is structural from day one.
- Email bodies are fetched lazily on first view, not during metadata sync.
- Rust never reads from GRDB except for initial state bootstrap via `readState()`.

## Assertions

| ID | Sev. | Assertion |
|----|------|-----------|
| cache-source-of-truth | MUST | UI reads only from local GRDB cache, never directly from JMAP |
| state-atomic | MUST | State strings are persisted in the same GRDB transaction as the data they describe via applySyncBatch |
| sync-idempotent | MUST | Replaying identical change deltas produces identical cache state |
| account-scoped | MUST | All cache tables include account_id in their primary key |
| body-lazy | MUST | Email bodies are fetched on first view, not during metadata sync |
| fallback-resync | MUST | On cannotCalculateChanges, engine performs full resync for affected type |
| ifInState | MUST | All mutations include ifInState; stateMismatch triggers resync, not blind retry |
| transaction-scope | MUST | applySyncBatch executes within a single GRDB transaction; writeEmailBodies uses its own transaction |
