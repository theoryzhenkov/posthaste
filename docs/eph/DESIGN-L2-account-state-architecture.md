---
scope: L2
summary: "Diagnosis and redesign of account state management: single-source-of-truth, runtime status coherence, incremental sync delivery"
modified: 2026-06-21
reviewed: 2026-06-21
lifecycle: ephemeral
type: DESIGN
depends:
  - path: docs/runtime/L2
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
- Message and smart-mailbox reads use `JOIN source_projection a ON a.source_id = m.account_id` (`store/read/messages.rs:16`, `smart_mailboxes/messages.rs:109`). The join is only for the display **name**, but it **gates message visibility**: if the projection row is missing/stale, the account's mail vanishes from every list, even though `message` rows exist.
- Account CRUD writes config then projection non-atomically (`config_delegates.rs:35-63`); a corruption-rebuild (new auto-repair) starts with an empty `source_projection`. Restart re-runs `service.sync_source_projections()` (`build.rs:185`), repopulating the projection — which is exactly why a restart "fixes" it.
- The account *overview* is config-backed (`account_reads.rs:56`) and stays, so what the user sees as "account disappeared" is its mail going blank.

### Mail doesn't arrive until the whole sync finishes
- Sync is batch-oriented end to end: the gateway fetches **all** messages into one `SyncBatch` (JMAP `sync/email.rs`, IMAP `gateway/execution.rs` accumulator), the store applies it in **one SQLite transaction** (`commands.rs:33`, `store.rs:187`), and the supervisor publishes events **only after the whole sync returns** (`sync_flow.rs:169`).
- Per-message `message.updated` events are created *inside* that transaction, so they're invisible until commit. The frontend already handles `message.updated` incrementally — it just never receives them until the end.

### Views don't update / state shown inconsistently
- Main app holds `queryKeys.accounts` with **`enabled: false`** (`MailClient.tsx:78`); invalidating it does **not** refetch, so many status events leave the main settings list stale.
- List renders from the `accounts` prop; the editor prefers `queryKeys.account(id)` — **two caches** that update at different times (`SettingsPanel.tsx:116`).
- `account.status_changed` is patched **all-or-nothing**: any missing/extra payload field downgrades to invalidation (`domain-cache/accounts.ts:63`), which then doesn't refetch in the main app.
- `set_runtime_overview` writes in-memory first and **swallows `append_event` failures** (`supervisor/shared.rs:132`), so status can change with no durable/broadcast event. Push-only changes don't emit `account.status_changed` at all.
- Supervisor has **no generation guard**: `stop_account` aborts without awaiting (`manager.rs:77`) and progress writes are spawned async (`sync_flow.rs:18`), so a stale runtime can overwrite a newer one's status.
- `RuntimeStatus.account_count` is computed **once** at startup and never updated (`build.rs:185`).
- Editor form state initializes **once** and never resets on account change (`AccountEditor.tsx:71`, `editorKeys.ts`). Progress meter only renders when `status === 'syncing'`, hides `0` values via truthiness, and prints raw enum text (`helpers/accountStatus.ts:21`, `AccountHeaderMeta.tsx`).
- Runtime + config fields share one `AccountOverview` DTO, so mutation-result merge, event patch, list fetch, detail fetch, and bootstrap hydration all race visibly.

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

## 5. Phased plan

**Phase 1 — Stop the bleeding (low risk, high impact).**
- `LEFT JOIN`/decouple message visibility from `source_projection`.
- Make the main-app accounts query observed so invalidations refetch.
- Tolerant `account.status_changed` patching.
- Fix progress-meter display bugs; humanize status text.
- Reset editor form on account identity change.

**Phase 2 — Supervisor coherence (medium).**
- Per-account generation/epoch guard against stale status writes.
- Durable-first status events (incl. push); stop swallowing append failures.
- Live `account_count`.

**Phase 3 — Incremental sync (larger, riskier).**
- Progressive apply + final deletion reconciliation; commit+publish per chunk.

**Phase 4 — Consolidate the account-state model (architectural).**
- One normalized account read model with explicit config-vs-runtime separation, removing the special-case merge logic.

## 6. Recommendation

Start **Phase 1** immediately — it directly fixes "mail/accounts disappear" and "views don't update" with localized, low-risk changes, and it's independently shippable. Phases 2–4 follow once Phase 1 is validated. Phase 3 (incremental sync) is the one change that needs careful design review before implementation because of the deletion-reconciliation correctness boundary.
