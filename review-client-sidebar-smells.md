# Sidebar / smart-mailbox / domain-cache — name-based-identity audit

Same class as the just-fixed `displaySmartMailboxName` / `smartMailboxPriority`
hacks: behavior derived by matching a mailbox/smart-mailbox display **name**
string when a stable `id` / `role` / `defaultKey` field already exists on the
read model.

## Key context: the stable fields already exist but get dropped at the prop boundary

`SmartMailboxSummary` (`api/types/smartMailboxes.ts:88`) carries three stable
identity fields that the sidebar ignores:

- `role: string | null` — "the mailbox role whose semantics apply to this view
  (e.g. 'trash')". `null` for All Mail / unassigned user smart mailboxes.
- `defaultKey: string | null` — "= its role for the built-in role mailboxes,
  `'all-mail'` for All Mail", `null` for user-defined. This is the exact field
  `resolveMailListPredicate` (`runtime/mailListSelfMaintained.ts:61`) uses as the
  single source of truth for built-in classification.
- `kind: 'default' | 'user'`.

`Mailbox` (`api/types/mail.ts:11`) carries `role` and source-mailbox rows already
use it correctly (`SidebarItems.tsx:141-143`). Smart-mailbox rows are the
regression: `SidebarContent.tsx:69-77` and `SmartMailboxItem`/`SmartMailboxSection`
thread only `name` (+ `id`, `unreadMessages`) into the icon/accent path, so the
presentation layer has no choice but to re-derive role from the name. The root
cause of findings 1–4 is this dropped-field prop boundary.

---

## HIGH — breaks on rename / locale / user-created duplicates

### H1. `smartMailboxAccent(name)` — accent color switched on lowercased display name
`mailboxRoles.tsx:82-115` (switch at `:84`, cases `:85-110`)
Call sites: `components/sidebar/SidebarContent.tsx:76`,
`components/sidebar/SidebarItems.tsx:103` (tags),
`components/settings-panel/SmartMailboxesPane.tsx:167`,
`command-search/providers/mailboxes.tsx` (via the same module).

- **Smell:** a 20-arm `switch (name.trim().toLowerCase())` (`'inbox'`,
  `'all inboxes'`, `'flagged'`, `'trash'`, `'bills'`, `'work'`, …) maps display
  name → accent. This is the direct sibling of the removed `displaySmartMailboxName`
  override and the `smartMailboxPriority` name partition.
- **Why fragile:** rename the built-in "Inbox" smart mailbox → it falls through to
  `muted`. Create a *user* smart mailbox literally named "Flagged" or "Bills" → it
  steals the built-in's coral/violet accent despite being unrelated. Any locale or
  user-chosen label that isn't in the hardcoded English list silently degrades. The
  list also encodes guesses (`'today'`, `'work'`, `'relevant'`, `'newsletters'`)
  that no built-in actually emits — pure name lottery.
- **SSOT fix:** drive built-in accents from `summary.role` (or `defaultKey`) via the
  existing `MAILBOX_ROLE_ACCENTS` map keyed on `KnownMailboxRole`; for role-less user
  smart mailboxes use a stable hash of `summary.id` (the pattern already in
  `model.ts:fallbackAccountAppearance`). The function currently only receives `name`,
  so it *cannot* see the stable field — change the signature to take the summary.
  Note: the **tag** call site (`SidebarItems.tsx:103`) is acceptable as-is — tags are
  name-identified in the domain (`TagSummary` has only `name`); split tag-accent off
  rather than forcing it through the smart-mailbox path.

### H2. Smart-mailbox icon derived by guessing a role from the name
`components/sidebar/SidebarItems.tsx:27-32` (`smartMailboxIcon`) →
`mailboxRoleFromName(name)` at `mailboxRoles.tsx:56-72`; rendered at
`SidebarItems.tsx:63`.

- **Smell:** `mailboxRoleFromName` is a heuristic `switch (name.toLowerCase())`
  (`'inbox'→Inbox`, `'spam'/'junk'→Junk`, `'trash'→Trash`, …) used *only* to pick the
  smart-mailbox icon, even though `SmartMailboxSummary.role` is the authoritative
  role and `defaultKey` the authoritative built-in key.
- **Why fragile:** the built-in Trash smart mailbox renamed to "Bin" loses its
  Trash2 icon; a user smart mailbox named "Archive" gets the Archive icon with no
  archive semantics; `'spam'` is hand-aliased to Junk but a localized spam label is
  not. The icon and the contextual-action role (`useSmartMailboxRole`, which reads the
  real `role`) can then disagree for the same row.
- **SSOT fix:** pass `summary.role` (or `defaultKey`) into `SmartMailboxItem` and feed
  it to `renderMailboxRoleIcon`; fall back to `Folder` for role-less. After this,
  `mailboxRoleFromName` has no remaining caller and should be deleted — it exists
  purely to paper over the dropped field.

### H3. `smartMailboxFallbackIcon` special-cases the literal "All Mail"
`mailboxRoles.tsx:127-128`: `name.toLowerCase() === 'all mail' ? Mail : Folder`.
Call sites: `SidebarItems.tsx:31`, `command-search/providers/mailboxes.tsx:28`.

- **Smell:** the only smart mailbox that should get the `Mail` icon is identified by
  its English display name.
- **Why fragile:** the All Mail smart mailbox has `defaultKey === 'all-mail'`
  (the value `resolveMailListPredicate:79` keys on). Rename or localize it → it
  reverts to the generic Folder icon. A user smart mailbox named "All Mail" wrongly
  gets the Mail icon.
- **SSOT fix:** key on `summary.defaultKey === 'all-mail'`, not the name.

---

## MEDIUM — latent fragility / drift risk

### M1. Stable fields dropped at the `SmartMailboxItem` prop boundary (the structural root of H1–H3)
`components/sidebar/SidebarContent.tsx:69-86` and `SidebarItems.tsx:34-63`.

- **Smell:** `SmartMailboxSection` has the full `SmartMailboxSummary` in hand
  (`SidebarContent.tsx:70`, used for `.id`, `.name`, `.unreadMessages`) but forwards
  only `id`, `name`, `unreadMessages`, and a pre-computed name-derived `accent`. It
  drops `role`, `defaultKey`, and `kind`, forcing the icon path to re-guess from the
  name.
- **Why fragile:** every name-based hack above is *enabled* by this narrowing; as long
  as `role`/`defaultKey` stop at the section boundary the leaf components have nothing
  else to key on.
- **SSOT fix:** pass the `SmartMailboxSummary` (or at least `role` + `defaultKey`)
  through to `SmartMailboxItem`, and derive icon + accent from those fields there.

### M2. Two independent classifiers for "is this a built-in role smart mailbox" (TS↔TS drift)
`runtime/mailListSelfMaintained.ts:36-83` (`ROLE_DEFAULT_KEYS` + `resolveMailListPredicate`,
keyed on `defaultKey`) vs. the sidebar's name-based role/icon/accent derivation
(`mailboxRoles.tsx` H1/H2/H3).

- **Smell:** the runtime already answers "what built-in is this smart mailbox" from
  `defaultKey` (the field designed for it, explicitly documented as the SSOT that
  "the store predicate and the runtime flag never drift"). The sidebar answers the
  same question independently from the *name*. This is exactly the dual-path
  derivation `resolveMailListPredicate` was introduced to collapse — except the
  presentation half never got migrated.
- **Why fragile:** the predicate layer correctly treats a renamed built-in inbox as an
  inbox (membership stays correct), while the sidebar shows it with a muted accent and
  Folder icon — the two layers visibly disagree about the same row's identity.
- **SSOT fix:** a single helper that maps `SmartMailboxSummary → { role, defaultKey }`
  and is consumed by both the predicate and the presentation layer, so icon/accent and
  membership cannot diverge.

### M3. Contextual-action role checks use raw role string literals instead of `MAILBOX_ROLES`
`actions/contextualActions.ts:57` (`role === 'trash' || 'archive' || 'junk'`),
`:98` (`viewRole !== 'archive' && viewRole !== 'trash'`), `:118` (`viewRole !== 'trash'`).

- **Smell:** identity here is correct (it keys on the canonical JMAP **role**, not a
  display name — good), but the role values are bare magic strings rather than the
  shared `MAILBOX_ROLES` vocabulary (`domainVocabulary.ts:10`) that the rest of the
  module graph uses (`useEmailActions.ts:263-267`, `mailboxRoles.tsx:21-35`).
- **Why fragile:** `viewRole` is typed `string | null`, so these comparisons get **no**
  type checking. If a role constant is ever renamed, `useEmailActions` (which uses
  `MAILBOX_ROLES.Trash`) updates but these literals silently rot — the menu would
  stop offering "Delete permanently" / "Move to Inbox" with no compile error.
- **SSOT fix:** compare against `MAILBOX_ROLES.Trash` / `.Archive` / `.Junk` so the
  literals are centralized and rename-safe.

---

## LOW — nits

### L1. `isUserTag` filters on the raw `$` prefix
`mailboxNavigationReadModels.ts:50-53`: `!name.startsWith('$')`.

- The `$`-prefix system-keyword convention is a legitimate JMAP/IMAP rule and tags are
  name-identified (no id field), so this isn't the same smell. Minor: the magic `'$'`
  could be a named constant (`SYSTEM_KEYWORD_PREFIX`) shared with wherever else system
  keywords are recognized, to keep the convention in one place.

---

## Clean (checked, no findings)

- `domain-cache/handlers.ts` and `domain-cache/invalidations.ts` — keyed entirely on
  `EVENT_TOPICS` constants and `queryKeys`; no name-based identity, no display-name
  special-casing.
- `hooks/useMailboxRole.ts` — both hooks resolve `role` from the cached read model by
  `id`; this is the correct SSOT and is what H1–H3 should adopt.
- `MailboxItem` (`SidebarItems.tsx:113-175`) — source-mailbox icon/accent already
  derive from `mailbox.role`, the right pattern.
- Sidebar selection union (`Sidebar.tsx:25-33`) — identity is `id`/`mailboxId`; `name`
  is carried only as a display label. Fine.
- `sortSmartMailboxes` (`model.ts:21-28`) — sorts by `position`, name only as a stable
  tiebreaker (not identity). Fine.

---

## Suggested fix order
1. **M1** (widen the prop boundary to carry `role`/`defaultKey`) — unblocks H1–H3.
2. **H2 + H3** (icon from `role`/`defaultKey`), then delete `mailboxRoleFromName`.
3. **H1** (accent from `role` + id-hash fallback; split the tag path off).
4. **M2** (shared `summary → {role, defaultKey}` helper consumed by predicate + UI).
5. **M3 / L1** (vocabulary constants) as low-risk cleanup.
