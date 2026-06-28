# Architecture Review: Runtime/Replica + Message-List/Mutation Layers

**Scope:** `apps/web/src/runtime/`, `apps/web/src/runtime/replica/`,
`apps/web/src/components/message-list/`, `apps/web/src/hooks/useEmailActions.ts`,
`apps/web/src/hooks/useRuntimeUndoRedo.ts`, `apps/web/src/app/MailClient.tsx`,
`MailPanels.tsx`, `MailClientView.tsx`, `MailClientView.types.ts`,
`useMailClientHandlers.ts`

**Date:** 2026-06-28  
**Type:** Review only — no fixes applied.

---

## 1. LEGACY PATHS THAT WORK AGAINST THE REPLICA

### [LOW] Dead `viewDelta` handler in `useRuntimeMailListView`
**File:** `apps/web/src/components/message-list/useRuntimeMailListView.ts:168–190`

The entity store adapter opens views **non-delta-capable** (option i — documented
in `entityStoreAdapter.ts:275–277` and class-level comment at line 55). The
session-level `viewDelta: true` flag (`sessionClient.ts:138`) opts the session
into deltas, but the entity store adapter never sends `viewDelta` frames — it
synthesizes `viewReplace` from the store's projection. For self-maintained
views, the runtime skips per-event re-serve (option iii). For deferred views,
the runtime sends full `viewReplace`. In neither case does a `viewDelta` frame
arrive.

The `case 'viewDelta'` handler at line 168 is therefore dead code. It adds
maintenance burden and implies a data path that no longer exists. The
`applyDeltaToQueryData` function (lines 56–81) is also dead.

**Fix:** Remove the `viewDelta` case + `applyDeltaToQueryData`. If delta-capable
views are re-introduced later, restore them with an explicit per-view opt-in.

### [MEDIUM] Resource-based invalidation path bypasses the entity-store guard
**File:** `apps/web/src/domain-cache/resources.ts:103–105`

```ts
case 'message':
  invalidateMessageListReadModels(queryClient)  // no skipStoreOwned
  return true
```

The `MessageUpdated` topic handler (`handlers.ts:120`) correctly checks
`isEntityStoreAdapterActive()` and passes `{ skipStoreOwned: true }` to avoid
redundant REST refetches. But `applyResourceInvalidation` for `kind === 'message'`
calls `invalidateMessageListReadModels(queryClient)` **without** `skipStoreOwned`.
Similarly, `case 'mailbox'` (line 107) calls `invalidateMailboxReadModels(queryClient, accountId)`
without `skipStoreOwned`.

Any event that carries a `resources` array with a `message` or `mailbox` resource
(e.g., `sync.completed`, `config.reloaded`, `account.created`) and uses
`applyResourceInvalidationsOrFallback` will take the resource path, bypassing the
entity-store guard. This causes redundant REST refetches of `messagesRoot` and
`mailboxes(accountId)` — exactly what the entity store was designed to prevent.

**Fix:** Thread `isEntityStoreAdapterActive()` into `applyResourceInvalidation`
and pass `{ skipStoreOwned: true }` for `message` and `mailbox` resource kinds
when the store is active.

### [MEDIUM] `invalidateSyncStartedReadModels` and `invalidateAccountReadModels` bypass the entity-store guard
**File:** `apps/web/src/domain-cache/invalidations.ts:36–46, 82–94`

`invalidateSyncStartedReadModels` (called from `MailClient.tsx:287` after every
manual sync) invalidates `queryKeys.messagesRoot` and `queryKeys.mailboxes(accountId)`
unconditionally — no `skipStoreOwned` check. When the entity store is active, the
`mailboxes(accountId)` invalidation triggers a REST refetch that overwrites the
store's `setQueryData` counts with a server round-trip. The `messagesRoot`
invalidation is harmless (the mail-list query is `enabled: false`), but the
mailbox refetch is redundant.

`invalidateAccountReadModels` (called from `AccountCreated`, `AccountUpdated`,
`AccountDeleted`, `SyncCompleted` fallback) also invalidates `mailboxes(accountId)`
and calls `invalidateMessageListReadModels(queryClient)` without `skipStoreOwned`.

**Fix:** Add `isEntityStoreAdapterActive()` checks to these functions, or pass
`skipStoreOwned` from their callers.

---

## 2. DUAL-SOURCE / DUPLICATED CONSTRUCTION

### [MEDIUM] `ROLE_DEFAULT_KEYS` hardcodes a duplicate of `KNOWN_MAILBOX_ROLES`
**File:** `apps/web/src/runtime/mailListSelfMaintained.ts:66–72` vs
`apps/web/src/domainVocabulary.ts:19–26`

```ts
// mailListSelfMaintained.ts
const ROLE_DEFAULT_KEYS = new Set([
  'inbox', 'archive', 'drafts', 'sent', 'junk', 'trash',
])
```

```ts
// domainVocabulary.ts
export const KNOWN_MAILBOX_ROLES = [
  MAILBOX_ROLES.Inbox, MAILBOX_ROLES.Archive, MAILBOX_ROLES.Drafts,
  MAILBOX_ROLES.Sent, MAILBOX_ROLES.Junk, MAILBOX_ROLES.Trash,
] as const satisfies readonly KnownMailboxRole[]
```

These are the same set, defined independently. If a new built-in role is added
to `KNOWN_MAILBOX_ROLES` but not to `ROLE_DEFAULT_KEYS`, the resolver would
treat the new role's smart mailbox as `'deferred'` (not self-maintained), causing
unnecessary runtime re-serves. The two would silently drift.

**Fix:** Import `KNOWN_MAILBOX_ROLES` and build the Set from it:
`new Set(KNOWN_MAILBOX_ROLES)`.

### [LOW] `defaultKey` vs `role` on `SmartMailboxSummary` — no consistency check
**File:** `apps/web/src/runtime/mailListSelfMaintained.ts:72–75` (uses `defaultKey`)
vs `apps/web/src/hooks/useMailboxRole.ts:44–48` (uses `role`)

The resolver uses `smartMailbox.defaultKey` to determine evaluability; the
contextual-action layer (`useSmartMailboxRole`) uses `smartMailbox.role` to
determine action availability. Both are separate fields on `SmartMailboxSummary`.
For a built-in "Inbox" smart mailbox, `defaultKey = 'inbox'` and `role = 'inbox'`
— consistent. But there's no client-side guard that they agree. If the server
sets `defaultKey = 'inbox'` but `role = null`, the store would self-maintain the
view (evaluable) but the contextual actions would show no role-specific actions
(no "Delete permanently" in Trash, no "Move to Inbox" in Archive).

**Fix:** This is primarily a server contract issue. On the client, consider
deriving `role` from `defaultKey` in one place, or asserting consistency in
development builds.

### [LOW] `'all-mail'` magic string with no constant
**File:** `apps/web/src/runtime/mailListSelfMaintained.ts:75`

```ts
if (key === 'all-mail') return 'all'
```

The `'all-mail'` defaultKey is a magic string. If the server changes the
defaultKey for All Mail, the resolver would fall through to `ROLE_DEFAULT_KEYS.has(key)`
→ false → `'deferred'`, silently breaking All Mail self-maintenance.

**Fix:** Extract a constant (e.g., `ALL_MAIL_DEFAULT_KEY = 'all-mail'`) shared
with the smart-mailbox type definitions or the sidebar model.

---

## 3. NAME-BASED IDENTITY / MAGIC-STRING SPECIAL-CASING

### [MEDIUM] `default-inbox` magic string for `DEFAULT_VIEW`
**File:** `apps/web/src/app/MailClient.tsx:37–40`

```ts
const DEFAULT_VIEW: SidebarSelection = {
  kind: 'smart-mailbox',
  id: 'default-inbox',
  name: 'Inbox',
}
```

The default view's smart-mailbox id is hardcoded as `'default-inbox'`. This is a
magic string that must match the server's smart-mailbox id for the built-in
Inbox. If the server's id scheme changes, the default view opens a non-existent
smart mailbox. The `name: 'Inbox'` is also hardcoded rather than derived from the
smart-mailbox read model.

**Fix:** Resolve the default view from the cached `smartMailboxes` list (e.g.,
find the smart mailbox with `defaultKey === 'inbox'`), falling back to the
hardcoded id only before hydration. This aligns with how the sidebar and
`mailListSelfMaintained` resolve identity from `defaultKey`.

### [LOW] `name` field carried in `SidebarSelection` but not used for identity
**File:** `apps/web/src/components/Sidebar.ts:25–30`

`SidebarSelection` carries a `name` field alongside the id. The `name` is used
for display (`ThreadListHeader` aria-label in `MessageList.tsx:215`) but is
duplicated from the smart-mailbox/mailbox read model. If the read model's name
changes, the `SidebarSelection.name` goes stale until the user re-selects the
view. This is a minor dual-source for display data, not identity — identity is
correctly driven by `id`/`sourceId`+`mailboxId`.

**Fix:** Consider deriving the display name from the read model at render time
rather than storing it in the selection state.

---

## 4. OUTBOX / SETTLEMENT CORRECTNESS

### [HIGH] View extend bypasses the entity store — race drops extended rows
**File:** `apps/web/src/runtime/replica/entityStoreAdapter.ts:369–371` (adapter
creation), `apps/web/src/components/message-list/useRuntimeMailListView.ts:281–296`

The entity store adapter overrides four methods: `openRuntimeSessionMessageListView`,
`closeRuntimeSessionView`, `subscribeRuntimeFrames`, `runRuntimeMutation`. It
does **not** override `extendRuntimeSessionView`. The adapter is created via
`{ ...deps.base, ...overrides }` (line 369), so `extendRuntimeSessionView`
delegates to the base HTTP adapter directly.

When `useRuntimeMailListView.loadMore` calls
`runtimeSessionClient.extendMessageListView(viewId, count)`:

1. The HTTP adapter returns a snapshot with the extended window.
2. `useRuntimeMailListView` applies it directly to the query cache
   (`applySnapshotToQueryData`, line 290–294) — the store's `setViewRowsJson` is
   **never called**.
3. The runtime later broadcasts a `viewSnapshot`/`viewReplace` frame through the
   SSE stream, which `onBaseFrame` handles (re-seeds rows + places them).

Between steps 2 and 3, the query cache has the extended rows but the store's view
only has the original page. If a `message.updated` event arrives in this window,
`drainAndEmit` → `emitChangedViews` projects the view with only the original
rows and emits a `viewReplace` that **replaces the extended rows with the
original page**, causing a visible row-loss/flicker.

**Fix:** Override `extendRuntimeSessionView` in the entity store adapter to call
`setViewRowsJson` with the extended snapshot's rows + watermark before returning.
Alternatively, have `useRuntimeMailListView.loadMore` not apply the snapshot
directly and instead wait for the broadcast frame.

### [MEDIUM] Outbox rehydration re-applies optimism but does not re-send
**File:** `apps/web/src/runtime/replica/entityStoreAdapter.ts:283–291`

```ts
for (const record of await this.deps.outbox.all()) {
  this.handle.acceptMutationJson(JSON.stringify({
    mutationId: record.clientMutationId,
    messageId: record.messageId,
    assertion: record.assertion,
  }))
}
```

On view open, the outbox is rehydrated: unconfirmed ops are re-accepted into the
store (optimism re-applied). But the mutations are **not re-sent** to the
runtime. If a mutation was persisted to the outbox but the page closed before
`base.runRuntimeMutation` was called (crash between `outbox.put` and
`base.runRuntimeMutation` at lines 314–322), the op is orphaned:

- Optimism is visible to the user (re-applied from the outbox).
- The server never receives the mutation.
- No `mutationNotification` ever arrives → the op never settles → the outbox
  record persists forever.

Even if the mutation was sent but the settlement frame was lost (session loss),
the op is orphaned: the server may have processed it, but the client never
receives the verdict.

**Fix:** After rehydrating, re-submit unconfirmed outbox records through
`base.runRuntimeMutation` (or a reconciliation endpoint that reports settlement
status by `clientMutationId`). At minimum, add a TTL or reconciliation sweep that
detects long-unsettled ops.

### [LOW] `clearRetired` is fire-and-forget — IndexedDB errors silently accumulate
**File:** `apps/web/src/runtime/replica/entityStoreAdapter.ts:335, 299, 310`

`void this.clearRetired()` is called from `onBaseFrame` for `viewSnapshot`/
`viewReplace` (line 299) and `notification` (line 310), and from `settleAll`
(line 335) with `await`. The fire-and-forget calls in `onBaseFrame` have no
error handling — if `outbox.remove` fails (IndexedDB quota, corruption), retired
records accumulate in the outbox without any log or retry.

**Fix:** Add a `.catch()` that logs the error and schedules a retry, or make
`clearRetired` resilient (idempotent drain + retry on next event).

### [LOW] No bounded growth for orphaned outbox records
**File:** `apps/web/src/runtime/replica/outboxStore.ts`

The outbox has no TTL, no cap, and no reconciliation. Records are removed only
when `drainRetiredJson` reports them retired. If settlements are lost (see
MEDIUM finding above), records persist indefinitely. Over many sessions with
lost settlements, the outbox grows without bound, and `outbox.all()` in
`openMailListView` loads all records on every view open, increasing latency.

**Fix:** Add a max-age or max-count sweep, or a reconciliation step on session
open that queries the runtime for settlement status of outstanding
`clientMutationId`s.

---

## 5. UI-STABILITY

### [HIGH] `isFetchingNextPage` hardcoded to `false` — pagination spinner never shows
**File:** `apps/web/src/components/MessageList.tsx:229`

```tsx
<MessageListRows
  ...
  isFetchingNextPage={false}   // ← hardcoded
  ...
/>
```

`useRuntimeMailListView` exposes `isLoadingMore` (line 306), but `MessageList`
passes `false` to `MessageListRows.isFetchingNextPage`. The bottom loading
spinner in `MessageListRows.tsx:96–100` is therefore never visible during
`loadMore`. Users get no feedback that more rows are being fetched — the list
appears frozen until the extended snapshot arrives.

**Fix:** Pass `isFetchingNextPage={runtimeMailListView.isLoadingMore}`.

### [MEDIUM] `placeholderData: (previous) => previous` shows stale rows from a different view
**File:** `apps/web/src/components/message-list/useRuntimeMailListView.ts:98–100`

```ts
const cached = useQuery<...>({
  queryKey,
  queryFn: () => { throw new Error('...') },
  enabled: false,
  placeholderData: (previous) => previous,
})
```

When switching views (e.g., Inbox → Trash), the queryKey changes. React Query's
`placeholderData: (previous) => previous` provides the prior view's data as
placeholder for the new query key. Until the new view's snapshot arrives, the
user sees the **previous view's rows** (Inbox messages) rendered under the new
view's header (Trash). This avoids a blank flash but creates a misleading
stale-snapshot — the user may attempt to act on rows that don't belong to the
current view.

**Fix:** Consider `placeholderData: undefined` (blank flash, but correct) or a
view-keyed loading state that suppresses row rendering until the new snapshot
lands. If the current behavior is intentional (product decision), document the
tradeoff in the hook.

### [LOW] Entity store swallows metadata-only `viewSnapshot`/`viewReplace` — `hasMore` can go stale
**File:** `apps/web/src/runtime/replica/entityStoreAdapter.ts:289–302, 351–361`

When `onBaseFrame` receives a `viewSnapshot`/`viewReplace` for a tracked view, it
updates `entry.lastSnapshot` (including `continuation`/`hasAfter`), re-seeds
rows, and calls `drainAndEmit()`. But `emitChangedViews` (line 351) only emits a
synthesized `viewReplace` when `projected.json !== entry.lastProjectionJson`
(i.e., rows changed). If the served frame changes only `continuation.hasAfter`
(rows unchanged), no synthesized frame is emitted, and the original frame is
**not passed through**. `useRuntimeMailListView` never sees the new `hasAfter`,
so `hasMore` stays stale.

In practice, `hasMore` is also set by `loadMore`'s direct response and by the
initial snapshot, so the gap only affects server-pushed continuation changes
(unlikely but possible after server-side window compaction).

**Fix:** Either pass through the original frame when the projection is unchanged
(so the renderer sees metadata updates), or update `hasMore` in the entity
store's synthesized path by always emitting when `entry.lastSnapshot` changed.

### [LOW] `scrollOffsetByView` Map grows unbounded
**File:** `apps/web/src/components/message-list/model.ts:10`

```ts
export const scrollOffsetByView = new Map<string, number>()
```

A module-level mutable Map with no eviction. Every unique `viewKey` (view +
search query + sort) adds an entry that is never removed. Over a long session
with many view switches and searches, this grows without bound. Entries are
small (string → number), but it's technically a memory leak.

**Fix:** Add an LRU cap (e.g., keep the last 50 view keys) or clear on session
end.

### [LOW] `afterSeq: 0` is misleading dead code
**File:** `apps/web/src/components/message-list/useRuntimeMailListView.ts:321`,
`apps/web/src/hooks/useRuntimeUndoRedo.ts:120`

Both hooks subscribe with `{ afterSeq: 0 }`. But `runtimeSessionClient.ensureStream`
is a no-op if the stream is already running (started by `useDaemonEvents` with
the stored sessionStorage seq). So `afterSeq: 0` is ignored. If
`useDaemonEvents` were ever not mounted (e.g., a page that renders only the
message list), `afterSeq: 0` would replay all frames from the session start —
potentially a large catch-up.

**Fix:** Pass `afterSeq: null` (or omit it) to clarify intent, or read the
stored seq from sessionStorage like `useDaemonEvents` does.

---

## 6. POWER / CAPABILITY GAPS

### [LOW] `addToMailbox`/`removeFromMailbox` have no named mutation — no optimism, no undo
**File:** `apps/web/src/runtime/mutations.ts:65–67`

```ts
case 'addToMailbox':
case 'removeFromMailbox':
  return null  // → legacy adapter path, no optimism
```

These commands fall back to `getRuntimeAdapter().runMessageCommand(request)` (the
legacy HTTP path), bypassing the entity store's `runMutation` and outbox. They
get no optimistic projection, no durable outbox record, and no undo/redo
integration. The comment says they "are not emitted by the action layer" — but
if they ever are, they'll silently lack the reactive-store benefits.

**Fix:** Add named mutations (`message.addToMailbox`, `message.removeFromMailbox`)
and assertions in the WASM `parseMessageMutation` when these commands are needed.

### [LOW] `sendMessage`/`saveDraft`/`deleteDraft` bypass the outbox — no offline queue
**File:** `apps/web/src/runtime/mutations.ts:159–168`

These go through `getRuntimeAdapter().sendMessage(...)` / `.saveDraft(...)` /
`.deleteDraft(...)` directly. The entity store adapter doesn't override these
methods (they're spread from `deps.base`), so they bypass the outbox entirely.
The reactive-store foundation could queue these for offline send/retry, but the
current wiring holds that back.

**Fix:** Route send/draft through `runtimeSessionClient.runMutation` with named
mutations, and extend `parseMessageMutation` to produce outbox records for
send/draft assertions.

### [LOW] Entity store doesn't feed message detail — offline detail is blocked
**File:** `apps/web/src/runtime/replica/entityStoreAdapter.ts`

The store ingests `message.updated` projections (full `MessageSummary`) but only
serves them as mail-list rows. Message detail is still fetched via REST
(`runtimeViews.mail.message` → `fetchMessage`) and invalidated by
`applyDomainEvent` → `invalidateTargetMessageReadModels`. The store's projection
could serve as a cache hit for message detail (or at least the summary fields),
enabling offline message reading. The current architecture treats the store as
list-only.

**Fix:** Consider a `messageDetail` view family that the entity store serves
from its projection cache, falling back to REST for body/attachment fields not
in the projection.

---

## Top 3 to Fix First

1. **[HIGH] View extend bypasses the entity store** (`entityStoreAdapter.ts` —
   missing `extendRuntimeSessionView` override). A `message.updated` event
   arriving between `loadMore` and the broadcast frame drops the extended rows,
   causing a visible row-loss/flicker. This is the most impactful race in the
   current architecture.

2. **[HIGH] `isFetchingNextPage` hardcoded to `false`** (`MessageList.tsx:229`).
   Users get no loading feedback during pagination. Simple one-line fix with
   immediate UX impact.

3. **[MEDIUM] Resource-based and sync/account invalidation paths bypass the
   entity-store guard** (`resources.ts:103–107`, `invalidations.ts:36–46,
   82–94`). Redundant REST refetches of mailbox counts fire on sync/account
   events even when the entity store owns them. This undermines the "single
   source of truth" design and causes unnecessary network traffic after every
   sync.
