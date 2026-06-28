# Client runtime/replica + message-list view layers — code-smell audit

Scope: `apps/web/src/runtime/**`, `apps/web/src/components/message-list/**`,
`MessageList.tsx`, `MessageRow.tsx`, `hooks/useEmailActions.ts`,
`app/Mail*.tsx`. Looking for the same class as the retired
`displaySmartMailboxName` / `smartMailboxPriority`: name-based identity hacks,
display-name overrides, dual-path view-descriptor/predicate construction,
scattered role/name special-cases, multi-site role derivation, and dead
branching on retired flags/names.

Review only — nothing changed.

The big single-source win (`resolveMailListPredicate` + `client_self_maintained`)
is genuinely intact: both the descriptor flag (`httpAdapter.mailListViewDescriptor`)
and the store predicate (`entityStoreAdapter.openMailListView`) derive from the
same `mailListSelfMaintained.ts` resolver. The findings below are the cousins
that survived.

---

## HIGH — breaks on rename / already diverged from the source of truth

### H1. `default-inbox` magic-string smart-mailbox id (sentinel that bypasses the id source of truth)
`app/MailClient.tsx:36-40`
```ts
const DEFAULT_VIEW: SidebarSelection = {
  kind: 'smart-mailbox',
  id: 'default-inbox',
  name: 'Inbox',
}
```
- **Smell:** the app's *initial and most-used* view is keyed by a hand-written
  string id that is never reconciled to a real server-assigned smart-mailbox id.
  Nothing in `MailClient`/`useMailClientHandlers` ever rewrites `selectedView`
  from `default-inbox` to the loaded inbox smart mailbox — it only changes on a
  user click (`useMailClientHandlers.ts:147`).
- **Why it's fragile (and already wrong today):**
  - `resolveMailListPredicate` → `ctx.smartMailboxDefaultKey('default-inbox')`
    returns `undefined` (no such id in the smart-mailbox list) → `'deferred'`.
    So the **default Inbox view is never `clientSelfMaintained`** and the runtime
    re-serves it per event — i.e. the headline view this whole entity-store
    effort optimizes silently takes the slow path until the user clicks a
    different mailbox.
  - `useSmartMailboxRole('default-inbox')` → `null`, so the default view's
    `viewRole` is `null` and `buildMessageContextActions` can't offer
    role-aware actions for it.
  - `SmartMailboxSection` highlights by `selectedView.id === smartMailbox.id`
    (`sidebar/SidebarContent.tsx`), so on first load **no sidebar row is
    selected**.
  This is the same shape as `displaySmartMailboxName`: behavior derived from a
  magic string instead of the entity's real id / `defaultKey`.
- **Single-source fix:** drop the sentinel. Resolve the default selection from
  the loaded smart-mailbox list — pick the summary whose `defaultKey === 'inbox'`
  (the existing stable field) once `smartMailboxes` hydrates, and hold "no
  selection" until then. Then the default view carries a real id → self-maintains,
  resolves its role, and highlights, with zero magic strings.

---

## MEDIUM — latent fragility / drift risk

### M1. `ROLE_DEFAULT_KEYS` re-lists the role vocabulary instead of deriving it
`runtime/mailListSelfMaintained.ts:36-43`, used at `:76`
```ts
const ROLE_DEFAULT_KEYS = new Set([
  'inbox','archive','drafts','sent','junk','trash',
])
...
if (!ROLE_DEFAULT_KEYS.has(key)) return 'deferred'
```
- **Smell:** a hand-maintained copy of `KNOWN_MAILBOX_ROLES`
  (`domainVocabulary.ts:19-26`, the canonical `['inbox','archive','drafts','sent','junk','trash']`).
- **Why fragile:** if a new built-in role/smart-mailbox `defaultKey` is added to
  the domain vocabulary, this set silently won't include it, so that view
  quietly degrades to `'deferred'` (always re-served) with no error — a perf
  regression that won't show up in tests keyed on the current six.
- **Single-source fix:** `const ROLE_DEFAULT_KEYS = new Set<string>(KNOWN_MAILBOX_ROLES)`
  (import from `domainVocabulary`). The `defaultKey`-for-built-in-role == role
  invariant is already documented in the file's header comment, so this is a
  pure de-dup.

### M2. Contextual-action role gates use raw role string literals
`actions/contextualActions.ts:56-58, 98, 118`
```ts
function isRestorableRole(role: string | null): boolean {
  return role === 'trash' || role === 'archive' || role === 'junk'
}
...
if (viewRole !== 'archive' && viewRole !== 'trash') { /* Archive */ }
...
if (viewRole !== 'trash') { /* Move to Trash */ } else { /* Delete permanently */ }
```
- **Smell:** the view-role → available-actions derivation is scattered across the
  builder as bare string literals, not `MAILBOX_ROLES.*`. This is exactly smell
  #4 (role-driven behavior keyed on magic strings).
- **Why fragile:** `viewRole` is typed `string | null` (widened all the way from
  `useMailboxRole`/`useSmartMailboxRole`, which return the raw `.role`), so a
  typo (`'achive'`) or a future role-token change compiles clean and silently
  drops/duplicates actions. The role vocabulary already exists as typed
  constants + an `isKnownMailboxRole` guard (`mailboxRoles.tsx`,
  `domainVocabulary.ts`).
- **Single-source fix:** compare against `MAILBOX_ROLES.Trash/Archive/Junk`, and
  type `viewRole` as `KnownMailboxRole | null` (narrow once at the
  `useMailboxRole`/`useSmartMailboxRole` boundary via `isKnownMailboxRole`).
  Optionally lift the "which roles enable which actions" table into one
  data-driven map rather than three inline guards.

### M3. The `${sourceId}:${id}` row-key format is re-derived in ≥4 places (cross-boundary contract)
- `components/message-list/model.ts:13` `messageKey` → `${message.sourceId}:${message.id}`
- `components/message-list/model.ts:17` `selectionKey` → `${selection.sourceId}:${selection.messageId}`
- `components/message-list/useRuntimeMailListView.ts:40` `rowKeyOf` → `${item.sourceId}:${item.id}`
- `runtime/replica/entityStoreAdapter.ts:140` `toStoreRow` → `${row.projection.sourceId}:${row.projection.id}`
- **Smell:** the same composite key is constructed independently in the renderer,
  the delta-reconcile, and the store-row mapping. The WASM store *emits* `rowKey`
  (`projectViewJson`, delta `upserts[].rowKey`) and the TS side reconciles by
  **recomputing** it (`applyDeltaToQueryData` matches `delta.order`/`upserts`
  keys against `rowKeyOf(heldItem)` — `useRuntimeMailListView.ts:64,70`).
- **Why fragile:** this is a TS↔(store/runtime) wire contract held together by
  four string templates agreeing. If any one drifts (or the store ever changes
  its key shape), `applyDeltaToQueryData` silently drops every held row whose
  recomputed key no longer matches an `order` entry — a flicker/empty-list bug
  with no type error. Same drift class you collapsed for the predicate.
- **Single-source fix:** one exported `messageRowKey(sourceId, id)` helper (the
  existing `messageKey` generalized to take the two ids) used by the renderer,
  `rowKeyOf`, `selectionKey`, and `toStoreRow` — and assert it matches the
  store's key construction in one boundary test.

### M4. Role→mailbox-id resolution implemented three times
- `runtime/httpAdapter.ts:160-168` `requiredMailboxByRole` (find one mailbox by role)
- `runtime/replica/entityStoreAdapter.ts` `roleMapForRequest` (build `role→id` map)
- `runtime/mailListSelfMaintained.ts:115-128` `buildMailListPredicateContext` (build `role→[ids]` across accounts)
- **Smell:** three separate walks over the cached `Mailbox[]` filtering on
  `mailbox.role`, each shaped slightly differently (one, map, multi-account map).
- **Why fragile:** the "resolve a role to its mailbox(es)" rule lives in three
  places; a change to how roles map to mailboxes (e.g. multiple mailboxes per
  role in one account, or honoring a precedence) must be made in all three or
  they diverge. `requiredMailboxByRole` is also on the legacy
  `moveMessageToMailboxRole` fallback while the live path uses the
  `message.moveToRole` named mutation + `roleMapForRequest`, so the two move
  paths can resolve the role differently.
- **Single-source fix:** one `roleIndex(mailboxes)` builder returning the
  `role → id[]` map; derive the single-id and per-account-aggregate views from
  it. Lower urgency than M1–M3 since the inputs are the same cache.

### M5. `scopeQuery` re-encodes the scope as a stringly-typed `in:` query DSL
`runtime/httpAdapter.ts:95-117` (and `mailQueryRequest` `:120-133`)
```ts
case 'source-mailbox':
  parts.push(`in:${request.scope.sourceId}/${request.scope.mailboxId ?? ''}`)
case 'smart-mailbox':
  parts.push(`in:${request.scope.smartMailboxId}`)
```
- **Smell:** the structured `RuntimeMessagePageScope` is flattened into a
  hand-built query string for the runtime-view path, in parallel with
  `fetchMessagePage` (`:380-405`) which dispatches the *same* scope structurally
  (`fetchSourceMessages` / `fetchSmartMailboxMessages` / `fetchSearchMessages`).
  Two encodings of one scope.
- **Why fragile:** the `in:` prefix, the `sourceId/mailboxId` slash join, and the
  empty-mailbox `?? ''` are an implicit contract with the backend query parser;
  it can't be type-checked and won't fail loudly if the parser's grammar shifts.
  This is the dual-path class, just split across transport DSLs rather than
  TS↔Rust.
- **Single-source fix:** if the runtime view path can take the structured scope
  directly (as `fetchMessagePage` does), drop the string encoding; otherwise
  isolate the scope→`in:` encoding in one tested function shared by both paths
  and pin the grammar with a contract test.

---

## LOW — nits / dead references

### L1. Dead comment referencing the retired mail-list feature flag
`components/message-list/useRuntimeMailListView.ts:256-259`
```ts
.catch(() => {
  // The legacy query path remains available by disabling the feature flag;
  // avoid broad invalidation/refetch here so this path stays targeted.
})
```
- **Smell:** references a "feature flag" / "legacy query path" that no longer
  exists — `MessageList.tsx:107-108` states the legacy HTTP-query + event-patch
  fork was retired, and there's no `runtimeMailListViewsEnabled` in the tree.
  Smell #5 (dead branching/commentary on a retired flag). The empty `catch` also
  swallows open-view failures silently, which the stale comment "explains" away.
- **Fix:** delete the comment; decide whether a failed `openMessageListView`
  should surface an error state instead of being silently swallowed.

### L2. `viewRole` derived by two parallel hooks, widened to `string`
`app/MailClient.tsx:90-99`
```ts
const sourceRole = useMailboxRole(...)
const smartRole = useSmartMailboxRole(...)
const viewRole = effectiveView?.kind === 'smart-mailbox' ? smartRole : sourceRole
```
- **Smell:** acceptable today (each entity is the source of its own `.role`), but
  the role is resolved as two hooks + a kind branch in the shell, then flows
  through ~5 prop layers (`MailClientView.types` → `MailPanels` → `MessageList`
  → `MessageListRows` → `MessageRow`) as `string | null`.
- **Fix (optional):** consolidate into one `useViewRole(selection): KnownMailboxRole | null`
  so the single resolution point is named and typed, and the contextual-action
  layer (M2) gets a narrowed type for free. Not a correctness issue.

### L3. Predicate/context built twice per view open
`runtime/httpAdapter.ts:144-149` (`isMailListSelfMaintained` + `buildMailListPredicateContext`)
and `runtime/replica/entityStoreAdapter.ts` (`resolveMailListPredicate` +
`buildMailListPredicateContext`) both run on a single mail-list open (the store
adapter wraps the base).
- **Smell:** the predicate is resolved twice from two independent
  `buildMailListPredicateContext(queryClient)` reads rather than computed once
  and threaded; the store adapter ignores the descriptor's `clientSelfMaintained`
  flag and recomputes.
- **Why low:** same inputs, effectively synchronous, and both go through the one
  resolver — so they can only disagree if the query cache mutates between the two
  calls (very unlikely). Worth noting as a latent divergence if either side ever
  gains async work between the calls.
- **Fix (optional):** have the store adapter read the predicate it already needs
  and pass `clientSelfMaintained = predicate !== 'deferred'` into the base
  descriptor, computing context once.

---

## Adjacent — same class, just outside the literal scoped dirs (flagged because the brief asks for the remaining `displaySmartMailboxName` cousins)

### A1. `mailboxRoleFromName` / `smartMailboxFallbackIcon` — name → role/icon heuristics (HIGH-class)
`mailboxRoles.tsx:56-73` and `:127-129`
```ts
export function mailboxRoleFromName(name: string): KnownMailboxRole | null {
  switch (name.toLowerCase()) { case 'inbox': ... case 'spam': return Junk ... }
}
export function smartMailboxFallbackIcon(name: string): LucideIcon {
  return name.toLowerCase() === 'all mail' ? Mail : Folder
}
```
- **Direct descendant of the retired `displaySmartMailboxName`:** derives a
  smart mailbox's role (and icon) by string-matching its **display name**.
  Consumed by `components/sidebar/SidebarItems.tsx:27-32` (`smartMailboxIcon`)
  and `command-search/providers/mailboxes.tsx:28`.
- **Why fragile:** `SmartMailboxSummary` already carries `defaultKey` *and* `role`
  (`api/types/smartMailboxes.ts:76-80`) — the stable source of truth. Keying off
  the localized/user-renameable name means renaming a smart mailbox, localizing
  it, or a user creating one literally named "Inbox" silently flips its
  icon/role. Breaks on exactly the rename axis the brief calls HIGH.
- **Single-source fix:** drive the sidebar/command-search icon from
  `smartMailbox.role` (fall through `renderMailboxRoleIcon(role, …)`), and the
  "All Mail" fallback from `defaultKey === 'all-mail'`, not the name. Thread
  `defaultKey`/`role` (already on the summary) into `SidebarItems` instead of
  passing only `name`.

These two are outside the directories you scoped (`components/sidebar/**`,
`mailboxRoles.tsx`, `command-search/**`), so I did not count them in the
in-scope ranking — but they are the clearest surviving `displaySmartMailboxName`
cousins and worth folding into the same cleanup.

---

## Quick reference (severity × file)

| Sev | Finding | Anchor |
|-----|---------|--------|
| HIGH | `default-inbox` sentinel id bypasses real smart-mailbox id (no self-maintain / no role / no highlight) | `app/MailClient.tsx:36-40` |
| MED | `ROLE_DEFAULT_KEYS` duplicates `KNOWN_MAILBOX_ROLES` | `runtime/mailListSelfMaintained.ts:36` |
| MED | contextual-action gates on raw role strings; `viewRole: string` | `actions/contextualActions.ts:56,98,118` |
| MED | `${sourceId}:${id}` row-key re-derived in 4 places (store↔TS contract) | `model.ts:13,17` · `useRuntimeMailListView.ts:40` · `entityStoreAdapter.ts:140` |
| MED | role→mailbox resolution implemented 3× | `httpAdapter.ts:160` · `entityStoreAdapter.ts` `roleMapForRequest` · `mailListSelfMaintained.ts:115` |
| MED | `scopeQuery` re-encodes scope as `in:` DSL in parallel to structural dispatch | `runtime/httpAdapter.ts:95` |
| LOW | dead "feature flag / legacy query path" comment + silent catch | `useRuntimeMailListView.ts:256` |
| LOW | `viewRole` two-hook derivation widened to `string` | `app/MailClient.tsx:90` |
| LOW | predicate/context built twice per open | `httpAdapter.ts:144` · `entityStoreAdapter.ts` |
| (adj) HIGH-class | `mailboxRoleFromName` / `smartMailboxFallbackIcon` name→role/icon | `mailboxRoles.tsx:56,127` |
