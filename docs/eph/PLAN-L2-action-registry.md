# PLAN-L2 — Unified Action Registry + Contextual Resolver

Status: proposal (read-only survey + design; no code changed).
Scope: `apps/web` mail-surface actions. Goal: ONE registry of action
definitions and ONE contextual resolver from which the command palette, the
right-click context menu, the focused-mail header actions, and the keyboard
map are all thin consumers — killing the four parallel definitions that exist
today.

Related: `docs/eph/DESIGN-L2-undo-redo-revlog-contract.md` (undo semantics the
handlers already obey), the Phase-0 seed in
`apps/web/src/actions/contextualActions.ts` (its header cites a
`PLAN-L1-contextual-actions.md` that no longer exists in `docs/eph/` — this
document supersedes it).

---

## 1. Current state — four surfaces, four definitions

### 1.1 The surfaces

| Surface | Entry point | How actions are defined |
| --- | --- | --- |
| Command palette | `apps/web/src/components/CommandPalette.tsx:52` | Provider pipeline: `command-search/providers/commands.tsx:94-189` hard-codes a `CommandPaletteEntry[]` inside `search()`; execution is a *second* switch in `components/command-palette/usePaletteActions.ts:29-127` over `PaletteAction` / `CommandActionId` |
| Right-click context menu | `components/MessageRow.tsx:104-107` renders `buildMessageContextActions()` from `actions/contextualActions.ts:70-159` | The Phase-0 registry seed: pure builder, role-gated availability, groups, `destructive` flag — but only `surface: 'context-menu'` (`contextualActions.ts:54`) and only reachable from a row |
| Focused-mail actions (detail header) | `components/message-detail/MessageHeader.tsx:162-348` (`HeaderActions`) | Hard-coded JSX buttons (Reply/ReplyAll/Forward/Archive/Trash/Snooze/Flag/Tag/Open; drafts get Edit/Discard at `MessageHeader.tsx:192-223` per D129) wired to per-prop callbacks |
| Keyboard | `components/keyboard/dispatch.ts:83-242` (pure dispatcher) fed by `keyboard/KeyboardController.tsx:61-173` | Key→callback map hard-coded as `if` chains; ~20 named callbacks on `KeyboardDispatchContext` (`dispatch.ts:30-64`) |
| (5th, passive) Shortcut reference | `components/ShortcutReference.tsx:14-32` | A static `SHORTCUTS` array, hand-maintained, already drifted (no ⌘K/⌘N/⌘R/⌘, chords listed) |
| (6th, passive) Top chrome | `components/ActionBar.tsx:97-171` | Global-only buttons (Compose, palette, `?`, settings, theme); titles hand-embed shortcuts ("Settings (⌘,)" at `ActionBar.tsx:153`) |

All of them converge on the same two handler layers:

- **Domain handlers**: `hooks/useEmailActions.ts:151-433` — `toggleRead`,
  `toggleFlag`, `setUserTags`, `archive`, `trash`, `moveToInbox`,
  `discardDraft` (D127/D134 grace + fold), `snooze`, `unsnooze`,
  `deletePermanently`; owns pending count (`isPending`, `useEmailActions.ts:155`),
  error state, and undo-toast wiring.
- **App handlers**: `app/useMailClientHandlers.ts:27-229` — selection-scoped
  wrappers (`handleArchive` guards on `selectedMessage`,
  `useMailClientHandlers.ts:123-127`; `handleTrash` reroutes drafts to discard,
  `:136-148`), navigation, overlays, compose intents.

`app/MailClient.tsx:193-285` then fans these out as ~25 props into
`KeyboardController` and ~40 into `MailClientView` (see
`app/MailClientView.types.ts`), and `app/MailOverlays.tsx:69-92` re-plumbs 15
of them into `CommandPalette`. This prop fan-out is itself a symptom: every
new action touches 5+ files.

### 1.2 The duplication table

| Action | Palette (commands.tsx / usePaletteActions.ts) | Context menu (contextualActions.ts) | Detail header (MessageHeader.tsx) | Keyboard (dispatch.ts) | Divergences |
| --- | --- | --- | --- | --- | --- |
| Archive | `commands.tsx:113-118` "Archive selected", gated by `hasSelectedMessage` (`:194`) | `builtin.archive` `contextualActions.ts:107-115`, hidden in archive/trash views | Button `MessageHeader.tsx:256-265` (always shown) | `e` at `dispatch.ts:225-227`, gated `hasSelectedMessage` | Only the context menu is role-aware; header offers Archive even in Archive/Trash; palette label says "selected" but acts on the single focused message |
| Trash / Delete | — (no palette entry at all) | Role-split: `move-to-trash` / `delete-permanently` / `discard-draft` `contextualActions.ts:127-156` | Trash button `:266-277` (drafts: Discard `:209-221`) | `#`/`Backspace` at `dispatch.ts:229-231` → `handleTrash` which re-implements the draft split (`useMailClientHandlers.ts:136-148`) | The draft-vs-message branching exists TWICE (contextualActions + useMailClientHandlers); trash-view "delete permanently" only reachable via right-click |
| Toggle read | — | `builtin.toggle-read` `contextualActions.ts:92-97` (label flips on `message.isRead`) | — | — | Right-click-only action |
| Flag | `commands.tsx:119-126` "Flag message" (icon is `Tag` — wrong icon, `commands.tsx:44-47`) | `builtin.toggle-flag` `:98-104` (Star, label flips) | Flag button `:308-320` (highlights when flagged) | `⌘⇧L` at `dispatch.ts:120-124` | Three labels ("Flag message"/"Unflag|Flag"/"Flag"), two icons (Tag vs Star vs Flag), chord not discoverable anywhere |
| Reply / Reply-all / Forward | Reply only, `commands.tsx:104-110` | — | All three, `:226-255` | `⌘R`/`⌘⇧R` `dispatch.ts:110-119`; no Forward key | Forward is header-only; palette lacks reply-all/forward |
| Snooze | `commands.tsx:127-134` → `noop` placeholder toast (`usePaletteActions.ts:113-115`) | — | Working preset popover `:278-307` → `actions.snooze` | — | Palette says "not available yet" for a shipped feature |
| Tag (open editor) | `providers/tagActions.tsx:21-53` (separate provider!) | — | Tag button `:321-333` | `t` at `dispatch.ts:233-237` | Palette needs a whole dedicated provider to add one contextual command |
| Open message / focused surface | `open-message` entries (messages provider) | `builtin.open` `:78-83` | Maximize button `:334-345` | `o` at `dispatch.ts:238-241` | Four independent wirings of `handleOpenFocusedMessage` |
| View conversation | — | `builtin.view-conversation` `:85-90` | — | `gc` prefix `dispatch.ts:182-187` | Two of four surfaces |
| Compose | `commands.tsx:96-102` | — | — | `⌘N` `dispatch.ts:105-109` | Plus a raw button in `ActionBar.tsx:116-120` |
| Settings / shortcuts / goto / undo-redo | settings+shortcuts entries `commands.tsx:143-188` | — | — | `⌘,` `:100-104`, `?` `:153-157`, `g…` `:174-200`, `⌘Z` `:129-138` | Goto targets (inbox/archive/trash/smart-by-role) are keyboard-only — invisible to the palette |

**Net:** archive/trash/flag/reply/open exist in 3-4 places each with divergent
labels, icons, gating, and even correctness (palette snooze is a stub, header
archive ignores view role). Enable/disable is computed four different ways:
`hasSelectedMessage` prop (palette, `commands.tsx:192-195`), `viewRole`
branching (context menu), optional-prop presence (header, e.g. `onTrash?`),
and `ctx.hasSelectedMessage` guards (keyboard, `dispatch.ts:224`).

### 1.3 Context that gates actions today

- **Selection**: single `MailSelection | null` (`selectedMessage`,
  `MailClient.tsx`). There is NO multi-select today (no `selectedIds`/selection
  set anywhere in `apps/web/src`), so the registry must model selection as a
  list from day one but ship with `length ∈ {0,1}`.
- **View role**: `viewRole` derived at `MailClient.tsx:109-117` from
  `useMailboxRole`/`useSmartMailboxRole` (`hooks/useMailboxRole.ts`); null for
  search/ambiguous views. Consumed only by `MessageRow` → contextualActions.
- **Draft-ness**: `message.keywords.includes(SYSTEM_KEYWORDS.Draft)` — checked
  in `contextualActions.ts:127`, `MessageHeader.tsx:64`, and
  `useMailClientHandlers.ts:140`.
- **Pane focus**: `PaneId = 'sidebar' | 'list'` (`dispatch.ts:19`), owned by
  `KeyboardController` and exposed via `usePane.ts`; detail pane is
  deliberately not focusable (`dispatch.ts:16-18`).
- **Input ownership**: `effectiveSurfaceOpen` (focused surface) and
  `overlayOwnsInput` (palette/compose/tag-editor/shortcuts) computed at
  `MailClient.tsx:194-201`, short-circuit the keyboard map
  (`dispatch.ts:88,171`).
- **Pending state**: `actions.isPending` (`useEmailActions.ts:155`) exists but
  NO surface consumes it for disabling — buttons stay clickable mid-flight.
- **Connection state**: not consulted by any action surface today (mutations
  fail into `errorMessage`); the resolver should carry a slot for it.

---

## 2. The unified model

### 2.1 Data model

New module: `apps/web/src/actions/` (grows the existing seed; the file layout
in §5 keeps `contextualActions.ts` as a re-export shim until Slice 4).

```ts
// actions/types.ts

import type { LucideIcon } from 'lucide-react'
import type { MessageSummary, SourceMessageRef } from '@/api/types'
import type { PaneId } from '@/components/keyboard/dispatch'

/** Where an action can appear. A definition opts into surfaces; the resolver
 *  filters by the requesting surface. */
export type ActionSurface =
  | 'palette'        // ⌘K list (searchable)
  | 'context-menu'   // right-click on a message row
  | 'detail-header'  // focused-mail action row (MessageHeader)
  | 'keyboard'       // dispatchable via shortcut

/** Section ordering within menus / palette groups. Supersedes the
 *  three-value ActionGroup in contextualActions.ts:30. */
export type ActionSection =
  | 'open' | 'compose-reply' | 'state' | 'organize' | 'move'
  | 'navigate' | 'app'

/** Serializable shortcut descriptor — replaces the if-chains in dispatch.ts.
 *  `key` is compared lowercased against KeyboardEvent.key. */
export interface ShortcutChord {
  key: string
  mod?: boolean      // metaKey || ctrlKey (matches dispatch.ts:90)
  shift?: boolean
  alt?: boolean
  /** Fires even when an editable element is focused (the "modifier chords"
   *  tier, dispatch.ts:94). Default false. */
  inEditable?: boolean
}

/** Everything the resolver knows at invocation time. Built fresh per event —
 *  cheap plain object, no hooks inside. */
export interface ActionContext {
  /** The action's subject(s). For the context menu this is the right-clicked
   *  row (which may differ from the focused message until onContextMenu's
   *  handleSelect lands, MessageRow.tsx:126); for keyboard/header/palette it
   *  is the focused selection. Length 0 or 1 today; multi-select later. */
  targets: MessageTarget[]
  /** Role of the current view (MailClient.tsx:116); null when ambiguous. */
  viewRole: string | null
  activePane: PaneId
  surface: ActionSurface
  /** Overlay/surface ownership — palette open, compose open, focused surface
   *  (MailClient.tsx:194-201). Global app actions stay available; message
   *  actions are suppressed while a surface owns the screen. */
  inputOwner: 'mail' | 'overlay' | 'surface'
  /** From useEmailActions.isPending — lets consumers render disabled/spinner. */
  hasPendingMutation: boolean
  /** Reserved: wire to daemon connection events later; 'unknown' ⇒ permissive. */
  connection: 'online' | 'offline' | 'unknown'
}

export interface MessageTarget {
  ref: SourceMessageRef
  /** Summary when the surface has it (row, cached detail); actions that flip
   *  labels (toggle-read) fall back to resolveKeywordState's cache path
   *  (useEmailActions.ts:88-105) when absent. */
  summary?: MessageSummary
  isDraft: boolean
  draftId?: string | null
  conversationId?: string
}

/** Injected once at registry-bind time (per render of the provider), NOT per
 *  action: the domain + app handler bundles that already exist. */
export interface ActionServices {
  email: EmailActions                       // hooks/useEmailActions.ts:41
  app: MailClientHandlers                   // app/useMailClientHandlers.ts return
  // navigation, overlays… added as slices migrate
}

export interface ActionDefinition {
  /** Stable namespaced id, e.g. 'message.archive', 'app.open-settings'.
   *  Persisted in recents/frequency counters — never rename casually. */
  id: string
  section: ActionSection
  /** Static title, or derived from context (toggle-read/flag flip labels the
   *  way contextualActions.ts:94,101 do). */
  title: string | ((ctx: ActionContext) => string)
  icon: LucideIcon | ((ctx: ActionContext) => LucideIcon)
  /** Search terms for the palette (commands.tsx `keywords` today). */
  keywords?: string
  surfaces: readonly ActionSurface[]
  shortcut?: ShortcutChord | readonly ShortcutChord[]
  /** Hidden entirely when false (context menu drops it; palette omits it). */
  isAvailable?: (ctx: ActionContext) => boolean
  /** Shown but not runnable; `disabledReason` renders as hint text. */
  isEnabled?: (ctx: ActionContext) => boolean | { reason: string }
  destructive?: boolean
  /** Confirmation before run — reuses the AlertDialog pattern
   *  (components/ui/alert-dialog.tsx; cf. account DangerSection.tsx:31 and
   *  ComposeCloseConfirmDialog.tsx). String = description copy. */
  confirm?: { title: string; description: string; confirmLabel: string }
  /** Handlers are thin: they call ActionServices, which already own
   *  optimistic folds, toasts, undo (useEmailActions dispatch, :167-204). */
  run: (ctx: ActionContext, services: ActionServices) => void | Promise<void>
}
```

Design choices, called out:

- **Availability vs enablement is split.** The context menu wants
  role-filtered *availability* (don't show "Archive" in Archive —
  `contextualActions.ts:107`); the palette wants *disabled-with-reason* for
  discoverability ("Archive — select a message first") instead of today's
  silent omission (`commands.tsx:192-195`). One predicate can't serve both.
- **Handlers get `services`, not closures.** Today `buildMessageContextActions`
  takes an `EmailActions` + two ad-hoc hooks (`contextualActions.ts:70-74`)
  and the palette re-declares 15 handler props (`usePaletteActions.ts:7-27`).
  Binding `ActionServices` once at the provider collapses the prop fan-out
  documented in §1.1.
- **`targets` is a list.** Multi-select doesn't exist yet (§1.3), but writing
  `run` against `ctx.targets` now means multi-select later is a resolver/UI
  change, not an every-action rewrite. Definitions that are inherently
  single-target declare it via `isEnabled: ctx => ctx.targets.length === 1`.
- **Titles/icons may be context functions** — this is what lets ONE
  `message.toggle-read` definition serve the flip-label context-menu item and
  a palette row, instead of two static entries.

### 2.2 Registry

```ts
// actions/registry.ts
const registry = new Map<string, ActionDefinition>()

export function registerActions(defs: readonly ActionDefinition[]): void {
  for (const def of defs) {
    if (registry.has(def.id)) throw new Error(`duplicate action ${def.id}`)
    registry.set(def.id, def)
  }
}
export function getAction(id: string): ActionDefinition | undefined
export function allActions(): readonly ActionDefinition[]
```

Module-level and populated at import time from static definition files
(`actions/defs/message.ts`, `actions/defs/app.ts`, `actions/defs/navigate.ts`)
— mirroring how `createCommandProviders` composes providers
(`command-search/providers.tsx:12-27`). No React in definition files (the
existing seed already enforces "icons are component references, not JSX",
`contextualActions.ts:9-10`); this keeps the registry unit-testable exactly
like `dispatch.ts` is today (`dispatch.ts:8` "pure makes it testable without a
DOM").

### 2.3 Resolver

```ts
// actions/resolve.ts
export interface ResolvedAction {
  def: ActionDefinition
  title: string          // context-applied
  icon: LucideIcon
  enabled: boolean
  disabledReason?: string
  /** Bound runner: applies confirm gating, then def.run(ctx, services). */
  execute: () => void
}

const SECTION_ORDER: readonly ActionSection[] = [
  'open', 'compose-reply', 'state', 'organize', 'move', 'navigate', 'app',
]

export function resolveActions(
  ctx: ActionContext,
  services: ActionServices,
  opts?: { includeDisabled?: boolean },   // palette: true; menu: false
): ResolvedAction[] {
  return allActions()
    .filter((d) => d.surfaces.includes(ctx.surface))
    .filter((d) => d.isAvailable?.(ctx) ?? true)
    .map((d) => bind(d, ctx, services))
    .filter((r) => r.enabled || opts?.includeDisabled)
    .sort(bySectionThenRegistrationOrder)
}
```

Pure given `(ctx, services)`. Section separators fall out of adjacent-section
changes exactly as `MessageRow.tsx:160-166` renders them today.

A thin hook adapts it to React:

```ts
// actions/useActionContext.ts — lives near MailClient, assembles ctx from
// selectedMessage/viewRole/activePane/isPending/input-ownership (all already
// computed in MailClient.tsx:109-201) and exposes:
//   buildContext(surface, targetsOverride?) => ActionContext
// plus a stable ActionServices. Provided via React context so MessageRow and
// MessageHeader stop threading `actions`/handler props.
```

### 2.4 Example definition (the role-gated trio, replacing contextualActions.ts:127-156)

```ts
// actions/defs/message.ts
export const messageActions: ActionDefinition[] = [
  {
    id: 'message.archive',
    section: 'move',
    title: 'Archive',
    icon: Archive,
    keywords: 'archive e',
    surfaces: ['palette', 'context-menu', 'detail-header', 'keyboard'],
    shortcut: { key: 'e' },
    isAvailable: (ctx) =>
      ctx.viewRole !== 'archive' && ctx.viewRole !== 'trash',
    isEnabled: (ctx) =>
      ctx.targets.length > 0 || { reason: 'Select a message first' },
    run: (ctx, s) => ctx.targets.forEach((t) => s.email.archive(t.ref)),
  },
  {
    id: 'message.trash',
    section: 'move',
    title: 'Move to Trash',
    icon: Trash2,
    destructive: true,
    surfaces: ['palette', 'context-menu', 'detail-header', 'keyboard'],
    shortcut: [{ key: '#' }, { key: 'backspace' }],
    // Draft split lives HERE once — deletes the twin logic in
    // useMailClientHandlers.ts:136-148 and contextualActions.ts:127-146.
    isAvailable: (ctx) =>
      ctx.viewRole !== 'trash' && !ctx.targets.some((t) => t.isDraft),
    isEnabled: (ctx) => ctx.targets.length > 0 || { reason: 'Select a message first' },
    run: (ctx, s) => ctx.targets.forEach((t) => s.email.trash(t.ref)),
  },
  {
    id: 'message.delete-permanently',
    section: 'move',
    title: 'Delete permanently',
    icon: Trash2,
    destructive: true,
    confirm: {
      title: 'Delete permanently?',
      description: 'This message will be destroyed. This cannot be undone.',
      confirmLabel: 'Delete',
    },
    surfaces: ['palette', 'context-menu', 'keyboard'],
    shortcut: [{ key: '#' }, { key: 'backspace' }],  // same keys; availability disambiguates
    isAvailable: (ctx) => ctx.viewRole === 'trash' && !ctx.targets.some((t) => t.isDraft),
    isEnabled: (ctx) => ctx.targets.length > 0 || { reason: 'Select a message first' },
    run: (ctx, s) => ctx.targets.forEach((t) => s.email.deletePermanently(t.ref)),
  },
  // message.discard-draft: isAvailable = every target isDraft; run =
  // s.email.discardDraft({...t.ref, draftId: t.draftId}) — the D127/D134
  // grace/undo machinery stays inside useEmailActions.ts:252-316 untouched.
]
```

Note the pattern for today's contextual trio: three *definitions* whose
`isAvailable` predicates partition the context — the resolver picks the right
one. Same for `message.toggle-read` (label/icon functions), `message.move-to-inbox`
(`isRestorableRole`, `contextualActions.ts:58-60`).

### 2.5 The consumers become thin

- **Context menu** (`MessageRow.tsx`): replace
  `buildMessageContextActions(actions, context, hooks)` (`:104-107`) with
  `resolveActions(buildContext('context-menu', [rowTarget]), services)`. The
  render loop (`:160-177`) is already shape-compatible (`title`, `icon`,
  `destructive`, group separators).
- **Detail header** (`MessageHeader.tsx`): `HeaderActions` maps
  `resolveActions(buildContext('detail-header'))` into icon `Button`s; the
  draft branch (`:192-223`) is deleted — draft-ness now gates via
  `isAvailable`. Snooze keeps its preset popover by rendering a custom control
  for `id === 'message.snooze'` (see §6, composite actions).
- **Palette**: the `commands` + `tag-actions` providers (`commands.tsx`,
  `tagActions.tsx`) collapse into one registry-backed provider (§4).
- **Keyboard**: §3.

---

## 3. Keyboard integration — shortcuts live on the definition

Today `dispatch.ts:83-242` hard-codes every chord and `KeyboardController`
threads 20 callbacks (`KeyboardController.tsx:36-59`). Unification:

1. **Add a registry tier to `dispatchMailKey`** between the goto machine and
   the pane handler. Precedence (`dispatch.ts:75-82`) is preserved:

   ```ts
   // dispatch.ts — new tier
   export interface RegistryKeyHook {
     /** Find a registered action matching this chord in this context. */
     match(event: KeyboardEvent, inEditable: boolean): (() => void) | null
   }
   ```

   The controller supplies `match` built from
   `resolveActions({ ...ctx, surface: 'keyboard' })`: filter definitions whose
   `shortcut` matches `(key, mod, shift, alt)`, take the first *enabled* one
   (availability disambiguates `#` in trash vs elsewhere, §2.4). Chords with
   `inEditable: true` are checked in the tier where `dispatch.ts:94-124` runs
   today; plain keys in the tier after `overlayOwnsInput`/pane-handler checks
   (`dispatch.ts:171,220-241`).

2. **What stays in dispatch.ts** (not actions — they're modes/navigation):
   the goto prefix machine (`:174-200`), pane rotation `⇧H/⇧L` (`:206-215`),
   pane handlers (j/k/h/l, `:220-221`), Escape-clears (`:140-151`), undo/redo
   (`:129-138` — tied to overlay-native-undo semantics). Everything else
   (⌘K, ⌘,, ⌘N, ⌘R, ⌘⇧R, ⌘⇧L, `?`, `/`, `e`, `#`, `t`, `o`) migrates to
   definitions and the corresponding branches are deleted. Goto *targets*
   (`gi`/`ga`/`gt`) additionally get palette-only `navigate.*` definitions so
   they become searchable without touching the prefix machine.

3. **`KeyboardDispatchContext` shrinks** from 20 callbacks to: the mode flags,
   pane fields, goto callbacks, undo/redo, clear-selection/search, and one
   `registryHook`. `KeyboardController` props shrink accordingly
   (`MailClient.tsx:193-227` loses most of its keyboard wiring).

4. **`ShortcutReference` is generated**: replace the static `SHORTCUTS` array
   (`ShortcutReference.tsx:14-32`) with `allActions().filter(a => a.shortcut)`
   grouped by section, plus a small static list for the non-action keys (j/k,
   ⇧H/⇧L, g-prefixes). One `formatChord(chord)` helper also feeds palette row
   shortcut hints and button `title`s (kills the hand-written "Settings (⌘,)"
   at `ActionBar.tsx:153`). Drift becomes impossible for action shortcuts.

Testing stays pure: `dispatch.test` gains cases that assert the registry tier
is consulted with the right precedence, and registry matching is tested
without a DOM (chord matching is data → data).

---

## 4. Palette enrichment

The palette already has real infrastructure the other surfaces lack:
provider fan-out with sections (`PaletteRow` sections, `types.ts:165-176`),
match evidence (`match.ts` — exact/prefix/acronym/fuzzy, `types.ts:63`), and a
feature-weighted ranker with recency/frequency slots
(`ranker.ts:92-94`). What's missing is *content* and *context*:

1. **One registry-backed provider replaces `commands.tsx` + `tagActions.tsx`.**
   `createActionProvider(getCtx)` maps
   `resolveActions(ctx(surface:'palette'), services, { includeDisabled: true })`
   to `CommandPaletteEntry`s. `PaletteAction` gains one variant —
   `{ kind: 'action'; actionId: string }` — and `usePaletteActions.ts`'s
   double-switch (`:29-127`) reduces to `getAction(id)` + `execute()`. The 15
   handler props threaded through `MailOverlays.tsx:74-89` collapse to the
   services bundle (navigation-entry kinds like `open-message`/`apply-query`
   stay as-is — they're search results, not actions).
2. **Disabled-with-reason.** Selection-scoped commands stop vanishing
   (`commands.tsx:192-195`) and instead render dimmed with
   `disabledReason` as the subtitle ("Reply — select a message first").
   `CommandPaletteEntry` gains `disabled?: boolean; disabledReason?: string`;
   `CommandPaletteList` renders and skips them on Enter.
3. **Contextual commands surface automatically.** Because availability is
   role-aware, a trash view's palette now offers "Delete permanently" and
   "Move to Inbox"; a draft offers "Edit draft"/"Discard draft" — all for free
   from the same definitions the context menu uses. This fixes the palette's
   current blind spots (no trash, no forward, no reply-all, stub snooze —
   §1.2).
4. **Shortcut hints on rows.** Entries carry `formatChord(def.shortcut)`;
   `CommandPaletteList` renders a right-aligned `<kbd>` — teaching keys at the
   point of use.
5. **Recents/frequency actually populated.** The ranker already weights
   `recentCommands`/`frequentCommands` (`ranker.ts:92-94`) but
   `createRankingContext` feeds it `emptyCounter()`s
   (`command-palette/model.ts:70-72`). Persist a `DecayedCounter`
   (`types.ts:117-120`) keyed by action id in localStorage, bumped in
   `execute()`. Stable action ids (§2.1) are what make this safe to persist.
   An empty-query palette then opens with a "Recent" section instead of
   nothing.
6. **Fuzzy search & grouping** need no new work — `match.ts` and section rows
   already exist; registry entries just flow through them with richer
   `keywords`.

---

## 5. Migration plan — five shippable slices

Ordering principle: registry lands silently first; each surface flips
independently; deletion of old paths happens per-slice, never big-bang.
Rollback for every slice is "revert the consumer, definitions are inert".

**Slice 1 — Registry + resolver + definitions (no visible change).**
Add `actions/types.ts`, `registry.ts`, `resolve.ts`, `defs/*.ts` with the
full message/app action set, mirroring current behavior exactly (including
today's per-surface quirks where they're deliberate, e.g. header shows
Archive regardless of role — normalize later, per-slice). Port
`contextualActions.ts` semantics into `defs/message.ts`; keep
`buildMessageContextActions` as a shim over `resolveActions` so `MessageRow`
is untouched. Unit tests: resolver filtering/ordering parity with the
existing builder (the seed is already pure and tested-shaped,
`contextualActions.ts:9-11`). Ship.

**Slice 2 — Context menu consumes the resolver directly.**
`MessageRow.tsx` builds an `ActionContext` (row target, `viewRole` it already
receives at `:45`, surface `'context-menu'`) via the provider hook; delete the
shim and `contextualActions.ts`. Row-scoped hooks (`onOpen`,
`onViewConversation`, `:73`) become `message.open` / `message.view-conversation`
definitions running through services. Visible change: none. Ship.

**Slice 3 — Palette.**
Add `createActionProvider`, remove `createCommandProvider` +
`createTagActionProvider` from `providers.tsx:20-24`, add the `action`
palette-action kind, shrink `usePaletteActions`. Land enrichment §4.2-4.4
here (disabled-with-reason, shortcut hints, contextual availability);
§4.5 recents as a fast-follow. Visible change: richer palette. Ship.

**Slice 4 — Detail header.**
`HeaderActions` renders from `resolveActions(surface:'detail-header')`;
delete the hand-rolled draft branch and per-action props
(`MessageHeader.tsx:47-62` slims; `MessageDetail.tsx:194` wiring shrinks).
Snooze popover stays a bespoke control keyed off the resolved action (§6).
Visible change: header becomes role-aware (no Archive in Archive) —
call this out in the changelog as a fix, not a regression. Ship.

**Slice 5 — Keyboard + shortcut reference.**
Add the registry tier to `dispatchMailKey` (§3), delete migrated branches and
`KeyboardDispatchContext` callbacks, generate `ShortcutReference` from the
registry. The prop fan-out in `MailClient.tsx:193-285` shrinks substantially.
Ship. (Optional Slice 6: populate `connection` and wire `hasPendingMutation`
disabling once product decides the semantics.)

Each slice deletes its old definition site; after Slice 5 the only sources of
action truth are `actions/defs/*` + `useEmailActions`/`useMailClientHandlers`
(which then become the `ActionServices` implementation and can be simplified
in place).

---

## 6. Risks and edge cases

- **Multi-select semantics (future).** `run` receives `targets[]`, but
  today's `EmailActions` are per-message mutations with per-message toasts
  (`useEmailActions.ts:206-224`) — naive fan-out would spam N toasts and N
  undo entries. When multi-select lands, add batched service methods and let
  definitions declare `multiTarget: false` to opt out. Toggle actions need a
  mixed-state rule (convention: if any target is unread → "Mark read" for
  all).
- **Async/pending.** Handlers are fire-and-forget through `dispatch`
  (`useEmailActions.ts:167-204`); `isPending` is global, not per-action.
  Disabling every action while any mutation is pending would feel broken
  under optimistic UI — recommendation: do NOT gate on
  `hasPendingMutation` initially (matches today), but keep it in ctx so
  destructive actions (`delete-permanently`) can opt in via `isEnabled`.
- **Destructive confirmation.** `confirm` metadata routes through one shared
  `AlertDialog` host (pattern of `ComposeCloseConfirmDialog.tsx` /
  `DangerSection.tsx:31`) mounted once in `MailOverlays`. Keyboard-invoked
  destructive actions get the same dialog — a behavior CHANGE for `#` in
  trash-view if we gate `delete-permanently` (today `#` only ever trashes;
  right-click delete-permanently is unconfirmed). Decide per-action:
  `discard-draft` keeps its toast-undo grace (D134) INSTEAD of a confirm —
  don't double-gate.
- **Context-menu target vs focused selection race.** `MessageRow` selects on
  `onContextMenu` (`MessageRow.tsx:126`) so target and selection converge,
  but the resolver must be built from the ROW's target, not
  `selectedMessage`, to be correct on the first right-click before state
  lands. Slice 2 must pass targets explicitly (§2.5), never read them from
  app state inside the menu.
- **Stale context in menus.** A resolved list is a snapshot; if a mutation
  changes `isRead` while a menu is open, the label is stale. Acceptable
  (today's behavior); `execute()` re-derives keyword state via
  `resolveKeywordState`'s cache path (`useEmailActions.ts:88-105`) so the
  *effect* is correct even when the *label* was stale.
- **Focus/pane races in keyboard tier.** The registry match must be computed
  inside the keydown using the same `stateRef` pattern
  (`KeyboardController.tsx:115-118`) — building `ActionContext` from render-
  scope state would reintroduce the race that ref exists to prevent. Keep
  `buildContext` a plain function over a ref snapshot.
- **Composite/parameterized actions.** Snooze needs a "until" parameter
  (preset popover, `MessageHeader.tsx:278-307`; palette entry currently a
  noop stub). Model as `message.snooze` opening a picker (header popover;
  palette can later add sub-entries per preset). Don't force parameters into
  the core `run` signature — a `run` that opens UI is fine.
- **Two actions, one chord.** `#` maps to both trash and delete-permanently,
  disambiguated by `isAvailable`. The resolver must guarantee at most one
  enabled match per chord per context — add a dev-mode assertion in the
  keyboard tier (overlapping availability predicates are a definition bug).
- **Testability.** Registry, resolver, and chord matcher are pure data → unit
  tests (same philosophy as `dispatch.ts:8`). Per-surface parity tests in
  Slice 1-2 (old builder output == resolver output for a matrix of
  viewRole × draft × selection) are the safety net for the whole migration.
- **Icon/label divergence resolution.** Unification forces choices (flag icon:
  `Star` vs `Flag` vs `Tag`, §1.2). Pick once in `defs/` and note in the
  slice changelog; don't try to preserve three inconsistencies.
