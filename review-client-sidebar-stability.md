# Sidebar / domain-cache / message-list — stability & architecture review

> Review-only audit of the Posthaste web client's sidebar/navigation/domain-cache
> + UI-stability surfaces. Workspace: `.workspaces/views-stability`. No files were
> edited. Findings cite `file:line` and group by the five requested problem classes.
>
> The name-based-identity findings (class 1) overlap with the prior
> `review-client-sidebar-smells.md` audit; they are re-cited here for completeness
> alongside the new UI-stability (class 3), invalidation-storm (class 2), and
> drift (class 4) findings that are the focus of this pass.

## Architecture context (verified from code)

- The reactive store (`runtime/replica/entityStoreAdapter.ts`) owns mail-list rows
  (synthesized view frames) AND mailbox counts (`writeMailboxCount` →
  `setQueryData(queryKeys.mailboxes(accountId))`, `entityStoreAdapter.ts:510`).
- The store does **not** write smart-mailbox or tag counts — those come from
  `SmartMailbox/list` / `Tag/list` REST via React Query (`mailboxNavigationReadModels.ts`).
- `isEntityStoreAdapterActive()` (`runtime/entityStoreState.ts:16`) is the live
  "store owns the mail list + mailbox counts" flag; the retired `VITE_ENTITY_STORE`
  opt-out is gone (`runtime/adapter.ts:162-163`).
- `resolveMailListPredicate` (`runtime/mailListSelfMaintained.ts:43`) is the single
  source of truth for the store predicate, keyed on `defaultKey` / `role` (stable),
  not names. `isMailListSelfMaintained` derives from it so the two cannot drift.
- `viewRole` for contextual actions is resolved **id-based** via
  `useMailboxRole` / `useSmartMailboxRole` (`hooks/useMailboxRole.ts`), then flows
  down `MailClient → MessageList → MessageListRows → MessageRow`.

---

## Class 1 — Name-based identity / display-name overrides

The stable fields `role` and `defaultKey` exist on `SmartMailboxSummary`
(`api/types/smartMailboxes.ts:88-105`) and are already used id-based by the
predicate resolver and the contextual-action role resolver. The sidebar
presentation layer drops them at the prop boundary and re-derives from `name`.

### 1a (HIGH) `smartMailboxAccent(name)` — accent keyed on lowercased display name
`mailboxRoles.tsx:82-115` (switch at `:84`).
Call sites: `components/sidebar/SidebarContent.tsx:76`,
`components/sidebar/SidebarItems.tsx:103` (tags),
`components/settings-panel/SmartMailboxesPane.tsx:167`.

A ~20-arm `switch (name.trim().toLowerCase())` maps `'inbox'`, `'all inboxes'`,
`'all mail'`, `'flagged'`, `'bills'`, `'work'`, … to accent colors. A user who
renames "Flagged" → "Important", a non-English locale, or a user-created smart
mailbox named "trash" all get the wrong (or muted) accent. The same `role` /
`defaultKey` fields the predicate resolver trusts are available here but ignored.
**Fix:** thread `role` + `defaultKey` into `SmartMailboxItem`/`SmartMailboxSection`
and key the accent off them (`mailboxRoleAccent(role)` for role-tagged boxes, a
`defaultKey`-keyed map for All Mail / built-ins).

### 1b (HIGH) `smartMailboxFallbackIcon(name)` — `"all mail"` name match for the Mail icon
`mailboxRoles.tsx:127`. Call sites: `components/sidebar/SidebarItems.tsx:31`,
`command-search/providers/mailboxes.tsx:28`.

`name.toLowerCase() === 'all mail'` picks the `Mail` icon; everything else gets
`Folder`. The All Mail smart mailbox has `defaultKey === 'all-mail'`
(`runtime/mailListSelfMaintained.ts:74,128`), which is exactly the stable key this
should branch on. Breaks on rename / locale.
**Fix:** branch on `defaultKey === 'all-mail'`.

### 1c (HIGH) `mailboxRoleFromName(name)` used to icon smart mailboxes
`mailboxRoles.tsx:56-58`, consumed at `components/sidebar/SidebarItems.tsx:29`
(`smartMailboxIcon(name)` → `mailboxRoleFromName(name)`).

Re-derives the role from the lowercased name (`'inbox'`, `'archive'`, …, plus
`'spam'` as a `junk` alias) solely to pick an icon — when `smartMailbox.role` is
already on the summary and is the field `useSmartMailboxRole`
(`hooks/useMailboxRole.ts:38`) resolves for the *action* layer. Two paths, same
role, one name-based and one id-based. Rename / locale / a user smart mailbox
named "inbox" all get the wrong icon.
**Fix:** pass `smartMailbox.role` into `SmartMailboxItem` and call
`renderMailboxRoleIcon(role, size, fallback)` directly; retire
`mailboxRoleFromName` for smart mailboxes (keep it only if a genuine
name-only fallback is still needed somewhere).

### 1d (LOW) Tags have no stable id — name-based accent is forced by the model
`TagSummary` (`api/types/mail.ts:152-156`) carries only `name` + counts; no `id`.
`SidebarItems.tsx:103` and the icon path therefore *must* key off `tag.name`. The
accent heuristics (`'bills'` → violet, `'newsletters'` → sage) still break on
rename, but there is no stable key to migrate to here.
**Note:** not fixable at the presentation layer; if tag identity matters, the
server `Tag` model needs an `id` (a separate, larger change). Flag for awareness.

---

## Class 2 — Legacy paths that work against the replica

### 2a (HIGH) `invalidateSyncStartedReadModels` — ungated store-owned invalidation
`domain-cache/invalidations.ts:36-47`. Caller: `app/MailClient.tsx:287`
(sync mutation `onSuccess`).

Unconditionally invalidates `queryKeys.messagesRoot` **and**
`queryKeys.mailboxes(accountId)` — both store-owned when
`isEntityStoreAdapterActive()`. Unlike `invalidateMessageListReadModels` /
`invalidateMailboxReadModels` (which take `{ skipStoreOwned }` and gate it,
`invalidations.ts:17-25`, `:95-101`), this function has no gate. On every
manual sync it fires a redundant REST mail-list refetch + mailbox-list refetch
that races the store's own SSE-fed updates — the "known ungated-invalidation-storm
item". This is the known ungated storm item.
**Fix:** pass `skipStoreOwned: isEntityStoreAdapterActive()` (or call the gated
helpers) so the store-owned keys are left alone when the store is active.

### 2b (HIGH) `invalidateComposeSendReadModels` — same ungated pattern
`domain-cache/invalidations.ts:49-63`. Caller:
`components/compose-overlay/useComposeSubmission.ts:46` (send mutation `onSuccess`).

Same problem as 2a for the send path: invalidates `messagesRoot` +
`mailboxes(accountId)` unconditionally. After a send, the store already folds the
new message into its rows + mailbox counts via the firehose; the REST refetch here
is redundant and can momentarily shadow the store's optimistic state.
**Fix:** gate on `isEntityStoreAdapterActive()` for the two store-owned keys.

### 2c (MEDIUM) Stale "legacy query path / feature flag" comment justifies swallowed errors
`components/message-list/useRuntimeMailListView.ts:256-258`:
```
.catch(() => {
  // The legacy query path remains available by disabling the feature flag;
  // avoid broad invalidation/refetch here so this path stays targeted.
})
```
The legacy REST mail-list fork **was retired** (`components/MessageList.tsx:99-101`
"the legacy HTTP query + event-patch fork was retired"; `runtime/adapter.ts:162-163`
"the prior `VITE_ENTITY_STORE` opt-out was retired"). There is no feature flag and
no legacy fallback for the mail list. The comment is stale and its rationale
("stay targeted because a fallback exists") no longer holds — the swallow now
hides a genuine failure with no recovery path (see 3b).
**Fix:** delete the stale rationale; surface the error to the view's error state
(or at least log + expose `retry`).

> Note: `runtime/useRuntimeObjectView.ts:15,99` also says "the legacy HTTP query
> for `queryKey` stays enabled" — that one is **accurate** (single-object detail
> views legitimately keep the REST fetch as initial load and layer runtime frames
> on top). Do not conflate the two; only the mail-list comment is stale.

### 2d (MEDIUM) `invalidateMailboxReadModels` always re-invalidates smart-mailboxes
`domain-cache/invalidations.ts:95-108`: even with `skipStoreOwned=true`,
`queryKeys.smartMailboxes` is invalidated unconditionally (`:101`), and
`invalidateMessageListReadModels` is called with `options` (skips `messagesRoot`
but still hits `conversationsRoot`).

In the `MessageUpdated` handler (`domain-cache/handlers.ts:120-160`), every event
with `arrived`/`mailboxes`/`keywords`/`deleted` calls this, so a sync burst of N
message events re-invalidates `smartMailboxes` N times. React Query dedupes within
a tick, but the smart-mailbox *counts* come from REST (`SmartMailbox/list`) and
therefore lag the store's instant mailbox-count writes (`entityStoreAdapter.ts:510`).
Result: in the same sidebar, source-mailbox unread counts update instantly while
smart-mailbox unread counts lag until the REST refetch settles — a visible
dual-source freshness gap (see 4c).
**Fix:** when the store is active, smart-mailbox *counts* could be store-fed too
(see 5a); until then, consider debouncing/coalescing the smart-mailbox
invalidation across a sync burst rather than per-event.

### 2e (MEDIUM) Full bootstrap re-fetched per message event
`invalidateMailNavigationBootstrapReadModels` (`invalidations.ts:31-35`) invalidates
`queryKeys.mailNavigationRead` — the 4-call composed bootstrap
(`mailboxNavigationReadModels.ts:42-67`: accounts + mailboxes + smartMailboxes +
tags). It is called on every `MessageUpdated` change-flag branch
(`handlers.ts:130,138,148,154`). During a sync this re-bootstraps the whole
navigation graph repeatedly. React Query dedupes rapid invalidations, but the
bootstrap is a single heavy composed call; per-event invalidation is wasteful and
contributes to the storm.
**Fix:** gate/defer bootstrap invalidation during sync bursts (e.g. invalidate on
`SyncCompleted` only, which `handlers.ts:62-69` already does for the same keys).

---

## Class 3 — UI-stability risks (flicker / blank-flash / stale-snapshot / selection)

### 3a (HIGH) `isFetchingNextPage={false}` — infinite-scroll spinner never renders
`components/MessageList.tsx:222` passes `isFetchingNextPage={false}` to
`<MessageListRows>`, even though `runtimeMailListView.isLoadingMore` is available
and **is** correctly wired to the scroll hook at `MessageList.tsx:154`
(`isFetchingNextPage: runtimeMailListView.isLoadingMore`). The bottom spinner at
`message-list/MessageListRows.tsx:92` (`{isFetchingNextPage && (...)}`) is
therefore dead — the user gets no feedback while a page extension is in flight.
**Fix:** `isFetchingNextPage={runtimeMailListView.isLoadingMore}` (one-line wire-up).

### 3b (HIGH) Silent view-open failure → stuck loading / stale rows, no error surfaced
`components/message-list/useRuntimeMailListView.ts:256-258` swallows the
`openMessageListView` rejection with `.catch(() => {})` (see 2c). Combined with:
- `isLoading` (`:139-142`) is true only when `cached.data === undefined`; with
  `placeholderData: (previous) => previous` (`:133`), `cached.data` is rarely
  `undefined` after the first view ever — so on a *return* visit a failed open
  shows the **stale snapshot with no loading indicator and no error**.
- `errorKey` is hardcoded `null` (`MessageList.tsx:135`), and `buildErrorState`
  only surfaces search-syntax errors (`MessageList.tsx:245-263`). A runtime
  view-open error is never surfaced; `onRetry` is wired (`MessageList.tsx:226`)
  but `errorState.showError` is never true for it.

On the very first load (no placeholder) a failed open leaves `isLoading=true`
indefinitely with no error and no retry affordance visible.
**Fix:** capture the open error into a view-local state, feed it to
`buildErrorState` (give it a real `errorKey`), and surface the retry banner.

### 3c (MEDIUM) Stale snapshot survives view close → no loading state on re-entry
`useRuntimeMailListView.ts:261-266` (effect cleanup) calls `closeView()` +
`setHasMore(false)` but does **not** clear the query cache entry for `queryKey`.
Returning to a previously-visited view finds the old `InfiniteData` still cached, so
`cached.data !== undefined` → `isLoading=false` immediately, and the old rows
render as if current until the new `viewSnapshot` lands. There is no version guard
on the cached snapshot in this hook (the version-guard ingest is replica-side only,
per commit `34cb41b3d`). The `placeholderData` trick is good for *cross-view*
switches, but *same-view re-entry* shows stale data with no indicator.
**Fix:** on cleanup, either `queryClient.removeQueries({ queryKey, exact: true })`
(or set a `stale` marker the render path can show as "refreshing"). Trade-off:
removing loses the instant-render feel; a "refreshing" overlay keeps it.

### 3d (MEDIUM) `loadMore` swallows extend errors; `hasMore` stays stale
`useRuntimeMailListView.ts:300` `.catch(() => {})` on
`extendMessageListView`. `isLoadingMore` resets via `.finally` so the (dead, per
3a) spinner would stop, but `hasMore` is left at its pre-extend value and no error
surfaces. If the runtime extend rejects, infinite scroll silently stops working
with no signal.
**Fix:** surface the extend failure (toast or a list-level retry) and recompute
`hasMore` from the error path.

### 3e (MEDIUM) Undo/redo `busyRef` can stick on a rejected mutation
`hooks/useRuntimeUndoRedo.ts:50-58` — `runApplyDiff` swallows mutation errors
(`.catch(() => {})` at `:56`) **without** clearing `busyRef`. `busyRef` is only
cleared when a `mutationHistory` frame arrives (`:100-106`). If the runtime
rejects the `message.applyDiff` mutation (e.g. transport error before the runtime
records a verdict) and never emits a `mutationHistory` frame for it, the queue
is permanently stuck — undo/redo stops responding to all subsequent keypresses.
**Fix:** clear `busyRef` in a `.finally` on `runApplyDiff`, or on a timeout, so a
missing frame can't wedge the queue.

### 3f (LOW) `scrollOffsetByView` — unbounded module-level Map
`components/message-list/model.ts:9`: `export const scrollOffsetByView = new
Map<string, number>()` accumulates a scroll offset per `viewKey` (which includes
sort + search query) and is never pruned. Grows with distinct search/sort
combinations across a long session; also mutable global state outside React.
Minor memory growth in practice, but it is a hidden singleton.
**Fix:** bound it (LRU cap) or move scroll restoration into the hook's local state
keyed by `currentViewKey`.

### 3g (LOW) Scroll restore races placeholder rows on view switch
`message-list/useMessageListScroll.ts:24-36` restores `savedOffset` for the new
`currentViewKey` as soon as `messageCount` updates. With `placeholderData`
showing the *previous* view's rows (3c) the restore can position the scroll
against rows that don't belong to the target view, then jump again when the real
snapshot lands. Minor visual judder; resolves once the snapshot applies.
**Note:** acceptable trade-off if 3c is addressed by clearing the cache on close.

---

## Class 4 — Dual-source / duplicated construction (TS↔Rust drift)

### 4a (MEDIUM) `contextualActions.ts` hardcodes role string literals
`actions/contextualActions.ts:57` (`'trash' || 'archive' || 'junk'`), `:98`
(`'archive'`, `'trash'`), `:118` (`'trash'`). These match `MAILBOX_ROLES.Trash` /
`.Archive` / `.Junk` (`domainVocabulary.ts:10-16`) by value today, but are bare
literals, not the constants. If the role vocabulary ever changes in
`domainVocabulary.ts`, these comparisons break silently — the action menu would
stop offering "Delete permanently" in Trash, etc.
**Fix:** import `MAILBOX_ROLES` and compare against `MAILBOX_ROLES.Archive` etc.

### 4b (LOW) `ROLE_DEFAULT_KEYS` duplicates the role vocabulary as literals
`runtime/mailListSelfMaintained.ts:38-44` hardcodes
`['inbox','archive','drafts','sent','junk','trash']` — the same set as
`KNOWN_MAILBOX_ROLES` (`domainVocabulary.ts:19-26`). The store predicate
classifier and the role-icon/accent maps could diverge if one is updated and the
other isn't.
**Fix:** derive from `KNOWN_MAILBOX_ROLES` (plus the `'all-mail'` defaultKey).

### 4c (MEDIUM) Sidebar counts are split store-owned vs REST-owned
- Source-mailbox counts: store-owned (`entityStoreAdapter.ts:510` writes
  `unreadEmails`/`totalEmails` into `queryKeys.mailboxes(accountId)`).
- Smart-mailbox + tag counts: REST-owned (`SmartMailbox/list`, `Tag/list`), only
  refreshed by invalidation.

Same sidebar, two count sources with different freshness (see 2d). The store
already computes message-list membership; it does not currently synthesize
smart-mailbox/tag counts.
**Fix (capability, see 5a):** have the store write smart-mailbox/tag counts via
`setQueryData` the same way it writes mailbox counts, so the whole count layer is
single-sourced and the per-event smart-mailbox invalidation (2d) becomes
unnecessary.

---

## Class 5 — Power / capability gaps held back by legacy paths

### 5a (MEDIUM) Smart-mailbox/tag counts still REST-owned → blocks full single-source counts
The reactive store already self-maintains evaluable predicates (source-mailbox,
built-in role smart mailboxes, All Mail under date sort) and writes mailbox counts.
Extending `writeMailboxCount`-style `setQueryData` to smart-mailbox and tag counts
would make the entire sidebar count layer store-fed: no REST lag, no per-event
`smartMailboxes` invalidation storm (2d), no count-freshness split (4c). The
legacy REST-refetch path is the only thing holding this back. This is an
enablement, not a bug — but it's the natural next retirement after 2a/2b.

### 5b (LOW) Undo/redo is message-diff scoped
`useRuntimeUndoRedo` builds `message.applyDiff` mutations only. The store
architecture (durable outbox + folded optimism) could support broader undo
(mailbox moves, flag batches), but the runtime history is scoped to message
changes. Not a blocker; a capability ceiling worth noting.

---

## Top 3 to fix first

1. **3a — `isFetchingNextPage={false}`** (`MessageList.tsx:222`). One-line wire-up
   of `runtimeMailListView.isLoadingMore`; restores the infinite-scroll spinner.
   Trivial change, immediate, visible UX win.
2. **2a + 2b — gate `invalidateSyncStartedReadModels` /
   `invalidateComposeSendReadModels` on `isEntityStoreAdapterActive()`** for the
   store-owned `messagesRoot` + `mailboxes(accountId)` keys. Stops the redundant
   REST refetch racing the store on every sync / send (the known ungated storm).
3. **1a + 1b + 1c — replace the three name-based smart-mailbox presentation
   functions with `role`/`defaultKey`-keyed maps**, threading `role` +
   `defaultKey` through `SmartMailboxItem` / `SmartMailboxSection` / the command
   palette. Eliminates the rename / locale / user-duplicate fragility in the
   sidebar's icon + accent.
