---
scope: L1
summary: "Sync loop, state tokens, SQLite schema, sync batch writes, conflict model"
modified: 2026-03-31
reviewed: 2026-03-31
depends:
  - path: spec/L0-sync
  - path: spec/L1-jmap
  - path: spec/L0-api
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

On trigger, the engine calls `*/changes` for each type with the stored state string, processes the deltas, and writes them directly to SQLite via rusqlite.

A single sync cycle proceeds in this order:

1. `Mailbox/changes(sinceState)` -- apply mailbox creates, updates, destroys
2. `Email/changes(sinceState)` -- apply email creates, updates, destroys
3. `Thread/changes(sinceState)` -- apply thread updates

Steps 1-3 can be batched in a single JMAP request (multiple method calls in one HTTP POST). After all deltas are applied, the engine persists new state strings in the same transaction via `apply_sync_batch`.

## State management

The Rust sync engine holds state strings in memory and persists them to SQLite directly. State strings are per-type, per-account. On startup, the engine reads initial state from SQLite -- Rust owns the database, so there is no bootstrap read from an external system.

If the server returns `cannotCalculateChanges` (state too old or server cannot compute the delta), the engine falls back to a full resync: `*/get` or `*/query` to rebuild the cache from scratch for the affected type. This is expected to be rare in normal operation but must be handled correctly because it will happen after extended offline periods.

## Sync batch writes

The primary write path is `apply_sync_batch`, which receives all changes from a sync cycle and executes them within a single SQLite transaction. State strings and data are persisted atomically, avoiding partial-sync corruption if the process crashes mid-write.

`write_email_bodies` is separate because body fetches happen outside the sync loop (on-demand when the user views an email).

```rust
fn apply_sync_batch(conn: &Connection, account_id: &str, batch: SyncBatch) -> Result<()> {
    let tx = conn.transaction()?;
    // Apply mailbox changes
    for m in batch.mailboxes_created { insert_mailbox(&tx, account_id, &m)?; }
    for m in batch.mailboxes_updated { update_mailbox(&tx, account_id, &m)?; }
    for id in batch.mailboxes_destroyed { delete_mailbox(&tx, account_id, &id)?; }
    // Apply email changes
    for e in batch.emails_created { insert_email(&tx, account_id, &e)?; }
    for e in batch.emails_updated { update_email(&tx, account_id, &e)?; }
    for id in batch.emails_destroyed { delete_email(&tx, account_id, &id)?; }
    // Apply thread changes
    for t in batch.threads_created { insert_thread(&tx, account_id, &t)?; }
    for t in batch.threads_updated { update_thread(&tx, account_id, &t)?; }
    for id in batch.threads_destroyed { delete_thread(&tx, account_id, &id)?; }
    // Persist new state strings
    save_state(&tx, account_id, &batch.new_state)?;
    tx.commit()?;
    Ok(())
}
```

## SQLite schema

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

## API response types

Rust structs serialized as JSON for the REST API. The sync engine writes raw JMAP data to SQLite; the API layer reads it back and serializes these response types for the frontend.

```rust
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MailboxResponse {
    id: String,
    name: String,
    role: Option<String>,
    unread_emails: u64,
    total_emails: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EmailResponse {
    id: String,
    thread_id: String,
    subject: Option<String>,
    from_name: Option<String>,
    from_email: Option<String>,
    preview: Option<String>,
    received_at: String,  // ISO 8601
    has_attachment: bool,
    is_read: bool,
    is_flagged: bool,
    mailbox_ids: Vec<String>,
    keywords: Vec<String>,
}
```

## Conflict model

All mutations include `ifInState` on `Email/set` and `Mailbox/set`. If the server returns `stateMismatch`, the engine re-syncs the affected type and presents the updated state to the UI. The original mutation is not retried automatically. The user sees the current state and can re-apply their action.

This is simple and correct for single-user mailboxes. Multi-user shared mailboxes would need a more sophisticated merge strategy, but that is out of scope.

## Error handling

```
SyncError
  |-- JmapError(JmapError)         -- protocol-level failure (wraps L1-jmap errors)
  |-- DatabaseError(rusqlite::Error) -- SQLite write failed
  |-- StateTooOld                   -- cannotCalculateChanges, need full resync
  |-- AccountNotFound(accountId)    -- unknown account
```

All errors are typed and explicit. The sync engine does not silently retry or swallow failures. `StateTooOld` (server's `cannotCalculateChanges` response) triggers a full resync for the affected type. Note: this is distinct from `StateMismatch` in L1-jmap, which is the `ifInState` precondition failure on mutations. `DatabaseError` is surfaced to the UI as a sync failure. `JmapError` is handled per the error model defined in L1-jmap.

## Invariants

- The local SQLite database is the single source of truth for the UI. The frontend reads via the REST API, never directly from JMAP.
- State strings are persisted atomically with data in a single SQLite transaction.
- A failed sync never leaves the database inconsistent. `apply_sync_batch` runs in a single transaction.
- Sync is idempotent: replaying the same change deltas produces the same cache state.
- All tables include `account_id`. Multi-account is structural from day one.
- Email bodies are fetched lazily on first view, not during metadata sync.

## Assertions

| ID | Sev. | Assertion |
|----|------|-----------|
| cache-source-of-truth | MUST | Frontend reads only via REST API, never directly from JMAP |
| state-atomic | MUST | State strings persisted in same SQLite transaction as data via apply_sync_batch |
| sync-idempotent | MUST | Replaying identical change deltas produces identical cache state |
| account-scoped | MUST | All cache tables include account_id in their primary key |
| body-lazy | MUST | Email bodies are fetched on first view, not during metadata sync |
| fallback-resync | MUST | On cannotCalculateChanges, engine performs full resync for affected type |
| ifInState | MUST | All mutations include ifInState; stateMismatch triggers resync, not blind retry |
| transaction-scope | MUST | apply_sync_batch executes within a single SQLite transaction |
