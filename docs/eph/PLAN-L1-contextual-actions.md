---
scope: L1
type: PLAN
lifecycle: ephemeral
summary: "A contextual-actions abstraction: one registry for built-in + user-defined actions on items, surfaced via right-click / palette / shortcuts, with contextual availability (e.g. Move to Inbox in Trash)"
modified: 2026-05-31
reviewed: 2026-05-31
depends:
  - path: docs/L1-ui
  - path: docs/L0-ui
dependents: []
---

# PLAN: Contextual actions

## Why

Actions on items are currently **hardcoded per surface**, and they are not
context-aware. The message-row right-click menu (`apps/web/src/components/MessageRow.tsx`)
lists a fixed set — Open, Mark read/unread, Flag/unflag, Archive, Move to Trash —
regardless of where the message lives. The motivating bug: inside **Deleted
Items / Trash** the menu still offers "Move to Trash" (a no-op) instead of
"Move to Inbox" (restore). Keyboard shortcuts (`e`/`#` in `MessageList`) and the
toolbar are *separate* hardcoded copies of the same intents, so they drift.

We want a single abstraction for **contextual, potentially user-defined actions
on items** that:

1. decides *which* actions apply and *how they are labelled* from the item's
   context (mailbox role, read/flag state, selection count, surface);
2. provides app **defaults** (Move to Inbox / Move to Trash / Archive / Flag /
   Mark read / Tag / Delete permanently …);
3. can later host **user-defined** actions — declarative presets ("mark as
   newsletter", "mark important"), small scripts, and external-code actions
   ("send this email to an LLM") — created through an **editor**;
4. feeds every surface from one place: right-click menu, command palette,
   keyboard shortcuts, and (future) a toolbar.

This document is a plan only. **Not implemented yet.**

## Current state (what we can build on)

- `apps/web/src/components/ui/context-menu.tsx` — reusable Radix context-menu
  primitives. The render layer already exists.
- `useEmailActions` (`apps/web/src/hooks/useEmailActions.ts`) already exposes the
  primitives every default action needs: `toggleRead`, `toggleFlag`, `archive`,
  `trash`, `deletePermanently`, `setTags`, and a **move-by-role** mutation
  (`mailboxRole: MAILBOX_ROLES.Inbox | Archive | Trash …`). "Move to Inbox" is
  already used today in the archive/trash **Undo** toast — restore is a solved
  primitive, just not surfaced as an action.
- Mailbox roles are typed: `KnownMailboxRole = inbox | archive | drafts | sent |
  junk | trash` (`apps/web/src/api/types.ts`). The current view carries its role
  for source-mailbox views.

So Phase 0 is mostly a **refactor + one new contextual rule**, not new backend
work.

## The abstraction

Two core types plus a resolver, all client-side to start.

### `ActionContext` — the resolved context handed to actions

```ts
interface ActionContext {
  targets: SourceMessageRef[]        // 1..n (multi-select is a later phase)
  primary: MessageSummary | null     // for label/state derivation
  view: {                            // where the user is
    kind: 'source-mailbox' | 'smart-mailbox' | 'search' | 'none'
    mailboxRole: KnownMailboxRole | null
    sourceId?: string
    mailboxId?: string
  }
  itemState: {                       // derived from primary/targets
    mailboxRole: KnownMailboxRole | null  // v1: from the view; later: per-message membership
    isRead: boolean
    isFlagged: boolean
    tags: string[]
    accountId: string
  }
  surface: 'context-menu' | 'command-palette' | 'keyboard' | 'toolbar'
}
```

### `ContextualAction` — a single action descriptor

```ts
interface ContextualAction {
  id: string                         // stable, namespaced: 'builtin.move-to-inbox', 'user.<uuid>'
  group: 'open' | 'state' | 'move' | 'tag' | 'custom'   // ordering + separators
  order?: number
  title: string | ((ctx: ActionContext) => string)      // dynamic label
  icon?: IconRef
  destructive?: boolean
  shortcut?: KeyChord                // optional keyboard binding (single source of truth)
  isAvailable(ctx: ActionContext): boolean   // contextual visibility
  isEnabled?(ctx: ActionContext): boolean    // shown-but-disabled (default: true)
  run(ctx: ActionContext): void | Promise<void>
  source: 'builtin' | 'user'
}
```

### Registry + resolver

- `ActionProvider = (deps) => ContextualAction[]` — built-in providers wrap
  `useEmailActions`; the user-action provider reads persisted definitions.
- `useContextualActions(ctx): ResolvedAction[]` — collects providers, filters by
  `isAvailable`, resolves dynamic titles, sorts by `group`/`order`, inserts
  separators. The context menu, palette, and keyboard map all consume this.

## Built-in default actions (the contextual matrix)

| Action | Available when | Effect (existing primitive) |
|---|---|---|
| Open | always (single target) | select |
| Mark read / unread | always | `toggleRead` |
| Flag / Unflag | always | `toggleFlag` |
| Archive | role ∈ {inbox, junk, sent, drafts, null} (i.e. not already archive/trash) | `archive` |
| **Move to Inbox** (restore) | role ∈ {trash, archive, junk} | move-by-role → `Inbox` |
| **Move to Trash** | role ≠ trash | `trash` |
| Delete permanently | role = trash | `deletePermanently` |
| Tag… | always | `setTags` |

This matrix is the fix for the reported bug: in Trash the menu shows **Move to
Inbox** + **Delete permanently**, not "Move to Trash". Generalises to archive,
junk, drafts, etc.

## User-defined actions (later phases)

A user action is a stored descriptor whose `run` is one of a typed
**`ActionEffect`**:

- `{ kind: 'op', op: 'move'|'setKeywords'|'tag'|'destroy', args }` — declarative,
  maps straight onto `useEmailActions`. Safe, no code. Covers "mark as
  newsletter" (add tag), "mark important" (flag/tag), "move to <mailbox>".
- `{ kind: 'script', source }` — user script run in a **sandbox** (e.g. QuickJS
  or a locked-down Web Worker) with **no ambient authority** — only a vetted,
  permissioned host API (read the message, add tags, move, etc.).
- `{ kind: 'external', target: 'http'|'llm', config }` — outbound calls ("send
  this email to an LLM"). Runs **server-side** (`posthaste-server`), never raw
  from the webview, so secrets/rate-limits/egress are controlled.

**Editor**: a form for declarative actions (title, icon, when-conditions builder
over `ActionContext`, effect) plus a code surface for scripted ones. Persistence:
local first (`localStorage`, like `developerTools`/column config), synced later.

## Surfaces — one registry, many entry points

- **Context menu** (Phase 0): `MessageRow` renders `useContextualActions(ctx)`.
- **Command palette** (Phase 1): same resolver, `surface: 'command-palette'`.
- **Keyboard** (Phase 1): actions own their `shortcut`; the `MessageList`
  keydown handler dispatches through the registry instead of its private
  `switch`. Kills the `e`/`#` drift.
- **Toolbar** (future): a chosen subset.

## Phasing

- **Phase 0 — skeleton + the bug** (small, safe, no backend): introduce the two
  types + `useContextualActions` with a built-in provider wrapping
  `useEmailActions`; refactor the `MessageRow` menu to render from it; ship the
  contextual move matrix (Move to Inbox in trash/archive/junk; Delete
  permanently in trash). Behaviour-preserving elsewhere.
- **Phase 1 — unify surfaces**: route the command palette + `MessageList`
  shortcuts through the registry; add multi-select context.
- **Phase 2 — declarative user actions**: `ActionEffect.kind = 'op'`, persisted,
  with an editor form. "Mark as newsletter / important / move to X."
- **Phase 3 — scripted + external actions**: sandbox, permission/capability
  model, server-side external effects (LLM/webhook), editor code surface.

## Design concerns / open questions

- **Security & privacy boundary (Phase 3)** is the hard part: executing user
  code and sending message content to external services (LLM) needs an explicit
  capability/consent model, a real sandbox (no raw network/file/DOM), and
  visible data-egress disclosure. Phases 0–2 must not bake in assumptions that
  block a clean sandbox boundary later. Relates to [[token-transport-decision]]
  and the capability-token design (`docs/eph/DESIGN-L1-capability-tokens.md`).
- **Item context for multi-mailbox messages**: JMAP messages can be in several
  mailboxes. v1 derives `itemState.mailboxRole` from the **current view**;
  per-message membership is a later refinement.
- **Single source of truth vs incrementalism**: shortcuts, menu, toolbar are
  separate today; migrate them onto the registry incrementally (Phase 1) rather
  than big-bang.
- **"Items" beyond messages**: keep `ActionContext` generic enough that the same
  registry can later target threads, accounts, mailboxes, or attachments
  (parameterise the target type) without a rewrite.
- **Where built-in op logic lives**: keep effects delegating to
  `useEmailActions` so optimistic-update/rollback/undo stays in one place.

## Spec impact when implemented

Update `docs/L1-ui` (the message-row context-menu paragraph and the keyboard
table) to describe the registry + contextual availability, and note the
Trash/Archive "Move to Inbox" + "Delete permanently" rules.
