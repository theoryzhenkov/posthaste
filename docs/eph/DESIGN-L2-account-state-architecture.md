---
scope: L2
summary: "Diagnosis and redesign of account state management: single-source-of-truth, runtime status coherence, incremental sync delivery"
modified: 2026-06-21
reviewed: 2026-06-21
lifecycle: ephemeral
type: DESIGN
depends:
  - path: docs/runtime/adapter/L2
  - path: docs/backend/L1
  - path: docs/stale/L1-accounts
  - path: docs/stale/L1-sync
---

# Account state architecture: diagnosis and redesign

## 1. Symptoms reported

- Account settings feel fragile; views don't always update.
- Accounts (or their mail) disappear after a sync until restart.
- Account state is displayed badly/inconsistently.
- During a full sync, mail doesn't arrive until everything is loaded.

## 2. Root cause: account state has four uncoordinated sources of truth

| Channel | Holds | Authority |
| --- | --- | --- |
| TOML config (`ConfigRepository`) | existence, name, enabled, connection settings, default | **authoritative** for config |
| `source_projection` (SQLite) | id → display name lookup | derived; **but message reads INNER JOIN it** |
| `AccountSupervisor.runtime_overviews` (in-memory) | status, push, sync progress, last error | volatile runtime truth |
| event log / broadcast | `account.created/updated/deleted/status_changed` | history + notifications |

These are written **sequentially, non-transactionally**, with no generation/version tying them together. Every reported symptom is a drift between two of these channels.

## 3. Symptom → cause map (evidence)

### Accounts/mail disappear after sync until restart
- **Fixed 2026-06-21:** message and smart-mailbox reads now use `LEFT JOIN source_projection` and fall back to `account_id` for display names. `source_projection` remains a repairable name projection, not a visibility gate.
- Previous failure mode: a corruption-rebuild or non-atomic config/projection update could leave `source_projection` empty; inner joins then hid all messages for that account until restart re-ran projection sync.
- The account *overview* is config-backed (`account_reads.rs`) and stayed visible, so what the user saw as "account disappeared" was its mail going blank.

### Mail doesn't arrive until the whole sync finishes
- Sync is batch-oriented end to end: the gateway fetches **all** messages into one `SyncBatch` (JMAP `sync/email.rs`, IMAP `gateway/execution.rs` accumulator), the store applies it in **one SQLite transaction** (`commands.rs:33`, `store.rs:187`), and the supervisor publishes events **only after the whole sync returns** (`sync_flow.rs:169`).
- Per-message `message.updated` events are created *inside* that transaction, so they're invisible until commit. The frontend already handles `message.updated` incrementally — it just never receives them until the end.

### Views don't update / state shown inconsistently
- **Fixed 2026-06-21:** `AccountOverview` now keeps config fields top-level and runtime state nested under `runtime`, so config mutations and status events no longer race through the same flat fields.
- **Fixed 2026-06-21:** the main-app `queryKeys.accounts` query is observed, so invalidations refetch instead of becoming no-ops.
- **Fixed 2026-06-21:** `account.status_changed` patching is tolerant/partial and writes into `account.runtime.*`.
- **Fixed 2026-06-21:** `set_runtime_overview` appends durable status events before updating the in-memory runtime overview and logs append failures instead of silently swallowing them. Push transitions are represented in the status payload and push events remain durable side effects.
- **Fixed 2026-06-21:** supervisor runtime writes carry a per-account generation guard, so stale progress/status writes from an aborted runtime are dropped.
- **Fixed 2026-06-21:** `RuntimeStatus.account_count` is derived live from the supervisor-owned account set when a supervisor-backed runtime is in use.
- **Fixed 2026-06-21:** progress display renders from `syncProgress` presence, handles `0` values, and humanizes status text. The account editor is keyed by account identity, so switching accounts resets form state without discarding edits during background refreshes.

## 4. Target design

Principle: **one authoritative model per concern, one read path, coherence enforced (not hoped for).**

1. **Config stays the only source of truth for account existence/settings.** (Already true.)
2. **`source_projection` must never gate visibility.** Decouple message reads from it — `LEFT JOIN` (or denormalize the source name onto `message`, or resolve names in the app layer). A missing projection row should at worst blank a name, never hide mail. Keep `sync_source_projections()` as a self-heal.
3. **Runtime status: single owner, single delivery, coherent.**
   - Add a per-account **generation/epoch**; tag runtime tasks and progress writes with it so a stale (aborted) runtime's writes are dropped.
   - Emit the durable `account.status_changed` event **as the source of the change** (don't write in-memory first and swallow the append). Include push transitions.
   - Derive `account_count` live from config.
4. **Frontend: one normalized account read path.**
   - Make the account list query actually observed (drop `enabled: false`, or drive everything from the event stream + one enabled query) so invalidations refetch.
   - Make `account.status_changed` patching **tolerant** (apply the fields present; never silently drop a live update).
   - List and editor read from one source; reset editor form when the account identity/version changes.
   - Separate config vs runtime in the cache (or DTO) so they stop racing.
   - Fix progress display: render from `syncProgress` presence, handle `0`, humanize labels.
5. **Incremental sync delivery.**
   - Apply + commit + publish in increments (per mailbox or per N-message chunk) so mail appears progressively.
   - Preserve full-snapshot deletion correctness by splitting sync into **progressive upserts** (stream in, commit+event per chunk) followed by a **final reconciliation** pass that prunes messages absent from the complete remote ID set. Additions stream; deletions reconcile at the end.

## 5. Implementation status

**Completed 2026-06-21**
- `LEFT JOIN`/decouple message visibility from `source_projection`.
- Nested runtime DTO: account config remains top-level, runtime state is `account.runtime`.
- Main-app accounts query is observed so invalidations refetch.
- Tolerant `account.status_changed` patching.
- Config mutation results preserve live runtime state.
- Progress/status display fixes.
- Per-account generation/epoch guard against stale status writes.
- Durable-first status events and visible append-failure logging.
- Live supervisor-backed `RuntimeStatus.account_count`.

**Remaining larger design item**
- Incremental sync delivery: progressive apply + final deletion reconciliation; commit+publish per chunk.

## 6. Recommendation

The account-state architecture is now coherent enough for dogfood/private-beta use. The remaining account-state work should focus on incremental sync delivery, which needs careful design review because deletion reconciliation is the correctness boundary.
