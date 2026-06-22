---
scope: L2
summary: "Account management as a coherent link: serve account state as a runtime view (snapshot, not deltas), and deliver sync progressively. Fixes stuck-syncing and batch-only mail arrival."
modified: 2026-06-22
reviewed: 2026-06-22
lifecycle: ephemeral
type: DESIGN
depends:
  - path: docs/replication/L1
    section: "5. Down-channel: authoritative assertions"
  - path: docs/runtime/L2
    section: "7. View operation flow"
  - path: docs/eph/DESIGN-L2-account-state-architecture
  - path: docs/stale/L1-accounts
  - path: docs/stale/L1-sync
---

# Account management as a coherent link

## 1. What this builds on

[DESIGN-L2-account-state-architecture](./DESIGN-L2-account-state-architecture.md)
removed the worst of the four-uncoordinated-sources problem: config is the only
authority for existence/settings, `source_projection` no longer gates mail
visibility, runtime overviews carry a per-account generation/epoch guard, and
status events are durable-first. That work made account state *coherent*. It did
**not** change how that state reaches the renderer, and two structural gaps
remain — both already solved for mail by the runtime-views migration (Phase
5b/5d), just never applied to accounts.

## 2. The two remaining gaps

### 2.1 Status is delivered as deltas the renderer patches (→ stuck "syncing")

The supervisor holds the authoritative runtime overview in memory
(`runtime_overviews`, generation-guarded). Every change emits an
`account.status_changed` **delta** event; the renderer patches that delta into
its `queryKeys.accounts` cache (`applyAccountStatusPatch` +
`mergeConfigPreserveRuntime`).

A patched delta stream has no convergence guarantee: if the renderer misses one
event — reconnect gap, a subscription that wasn't live at emit time, a dropped
broadcast — its cached status is stranded while the supervisor's overview is
correct. That is exactly the reported residual: an account sits in **syncing**
until you press sync manually, because a fresh sync emits a fresh delta the
renderer now happens to catch. The supervisor was never wrong; the *delivery*
was lossy.

This is the same failure the renderer-views work eliminated for mail. The fix is
the same: **serve the current snapshot, don't replay deltas.**

### 2.2 Sync delivers in one batch (→ mail doesn't arrive until it finishes)

The gateway accumulates **all** messages into one `SyncBatch`; the store applies
it in **one** SQLite transaction; the supervisor publishes events **only after
the whole sync returns** (`sync_flow.rs`). The per-message `message.updated`
events exist inside that transaction and are invisible until commit. The
renderer already applies them incrementally — it just never receives them until
the end. During a large initial sync the mailbox stays empty, then fills all at
once.

## 3. Principle

Account state is a **coherent link** like every other surface
([replication L1](../replication/L1.md)): the runtime is the authority; the
renderer renders the runtime's **served snapshot**; changes propagate as
**base updates (re-served snapshots), never as deltas the renderer must not
miss**; and the authoritative down-channel applies **progressively**, not in one
shot.

Two pillars realize this for accounts.

## 4. Pillar A — Accounts as a served runtime view

Implement the **`accountStatus` view family** already specified in runtime/L2
§5.7 (currently spec'd but unimplemented), peer to `mailList`/`messageDetail`/
`conversation`. Its all-accounts variant serves the list the renderer already
consumes: `Vec<AccountOverview>` — config folded with the supervisor's runtime
overview (the same fold `account_reads.rs` already performs). A per-account
variant (account id) can back the editor.

- **Open:** the renderer opens one `accounts` view per session; the snapshot is
  the full current list (config + live runtime status). Reconnect/late-subscribe
  always yields the current snapshot — a missed update is structurally
  impossible.
- **Recompute triggers:** any of `account.created`/`updated`/`deleted` (config)
  or a runtime-overview change (status, push, sync progress, last error)
  recomputes the view and emits a `ViewReplace`. The supervisor already
  centralizes overview writes in `update_runtime_overview`; that commit point
  becomes the single recompute signal.
- **Data-equality suppression:** reuse the views registry's existing
  recompute-if-changed so identical recomputes don't churn frames.

**Renderer:** the accounts list, the editor, and the directory read from the
view frame (mirroring `useRuntimeObjectView`). Retire `applyAccountStatusPatch`,
`mergeConfigPreserveRuntime`, and the `account.status_changed` /
`account.updated` cache-patch handlers. `queryKeys.accounts` is seeded and kept
fresh by the view, exactly as the mail surfaces are.

**Why this fixes it:** status is a snapshot every consumer re-renders, not a
patch any consumer can drop. Stuck-syncing cannot occur; multi-window
consistency is automatic; the editor-vs-list divergence disappears (one served
source).

**Scope note:** the `account.status_changed` *durable event* stays (it is the
status audit trail and survives restart); what is retired is the renderer
**depending on receiving every delta**. The view is the read path; the event log
is history.

## 5. Pillar B — Incremental sync delivery

Restructure one sync cycle from "fetch-all → apply-all → publish-all" into a
**progressive upsert stream followed by a final deletion reconciliation**.

1. **Stream chunks.** The gateway yields the sync as chunks — naturally per
   mailbox, or per N messages within a mailbox — instead of accumulating one
   `SyncBatch`. (JMAP `sync/email.rs`, IMAP `gateway/execution.rs` accumulators
   become generators.)
2. **Apply + commit + publish per chunk.** The store applies each chunk in its
   own transaction; the supervisor publishes that chunk's `message.updated`
   events immediately. Mail appears progressively; the `accounts` view's
   `syncProgress` advances per chunk (Pillar A makes that visible live).
3. **Final reconciliation = the correctness boundary.** Additions and updates
   stream safely (upserts are idempotent). **Deletions cannot** be inferred
   mid-stream — a message absent from chunk *k* may appear in chunk *k+1*. So a
   full sync ends with one reconciliation pass: prune local messages absent from
   the **complete** remote ID set gathered across all chunks, in a final
   transaction. Incremental syncs (delta/CHANGES-based) already carry explicit
   removals and skip the reconciliation pass.

**Failure semantics:** a sync that fails mid-stream has already committed +
published its earlier chunks (progress, not loss); it simply does **not** run
the deletion-reconciliation pass, so nothing is wrongly pruned from a partial
view. The next full sync reconciles. This is strictly safer than today's
all-or-nothing transaction, which discards a partial sync entirely.

**Interaction with local-first:** the flush → observe → retire cycle
([replication L1](../replication/L1.md)) is unchanged — it still flushes pending
ops before the pull and retires confirmed assertions after. Progressive chunks
just move the "after" convergence earlier and per-chunk; pending overlay
assertions fold over each committed chunk the same way.

## 6. What stays as-is (already coherent)

- Config is the sole authority for existence/settings.
- `source_projection` is a repairable name projection, never a visibility gate.
- Per-account generation/epoch drops stale runtime writes.
- Durable-first status events; live supervisor-backed `account_count`.
- Lifecycle (start/stop/remove/enable/disable) and OAuth refresh.

## 7. Slices

Each slice is independently shippable and verifiable.

- **A1 — server `accountStatus` view family.** Generalize the view registry to
  serve `Vec<AccountOverview>`; wire `update_runtime_overview` + account config
  events as recompute triggers; tests for status/config/delete recompute +
  equality suppression.
- **A2 — renderer reads the view.** `useAccountsView` (mirrors
  `useRuntimeObjectView`) feeds `queryKeys.accounts`; list/editor/directory
  consume it.
- **A3 — retire delta patching.** Delete `applyAccountStatusPatch`,
  `mergeConfigPreserveRuntime`, and the account cache-patch handlers; keep the
  durable event log.
- **B1 — gateway chunk stream.** Turn the JMAP/IMAP accumulators into chunk
  generators behind a sync-chunk callback; no behaviour change yet (one chunk).
- **B2 — store apply-per-chunk + supervisor publish-per-chunk.** Commit and emit
  per chunk; the view shows progressive arrival.
- **B3 — final deletion reconciliation.** Gather the complete remote ID set
  across chunks; prune in a final transaction on full syncs only; deletion
  correctness tests (message present late, sync fails mid-stream, full vs
  incremental).

A1–A3 (status snapshot) land first — they fix the user-visible stuck-syncing and
are lower-risk. B1–B3 (progressive sync) follow; B3 is the one that needs the
most test care.

## 8. Open questions / decisions for review

1. **Chunk granularity for B.** Per-mailbox is the natural seam and bounds
   reconciliation per mailbox; per-N-messages gives smoother progress but a
   larger reconciliation set. Recommend **per-mailbox**, with a soft cap that
   splits very large mailboxes.
2. **Accounts view scope.** Session-scoped to the caller's account set (like
   mail views), or one global accounts view? Settings shows all accounts, so a
   global (unscoped) `accounts` view is simplest; confirm that matches the
   capability model.
3. **Does the renderer still need `queryKeys.accounts` as a query at all,** or
   should consumers read the view-backed cache directly? Keeping the query key
   (seeded by the view) is the least invasive and matches how mail surfaces
   coexist with their HTTP queries; recommend keeping it.
4. **Restart cold-start status.** After restart, before the first sync, accounts
   serve `Offline`/`Idle` from a fresh overview. Confirm that's the desired
   resting state (vs. replaying the last persisted status event).
