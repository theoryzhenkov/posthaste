/**
 * Unified action registry — core types (PLAN-L2, Slice 1).
 *
 * ONE definition per message/mail action, from which every surface (palette,
 * context menu, detail header, keyboard) will eventually resolve. Slice 1 lands
 * the machinery and ports the pure, role-gated context-menu actions from
 * `contextualActions.ts` WITHOUT changing any user-visible behavior — the old
 * builder becomes a shim over {@link resolveActions}.
 *
 * Definitions are pure data: titles/icons are values or `(ctx) => value`
 * functions, never JSX, so the whole registry is unit-testable without a DOM
 * (same philosophy as `components/keyboard/dispatch.ts`).
 *
 * @spec docs/eph/PLAN-L2-action-registry.md
 */
import type { LucideIcon } from 'lucide-react'
import type { MessageSummary, SourceMessageRef } from '../api/types'
import type { PaneId } from '../components/keyboard/dispatch'
import type { useMailClientHandlers } from '../app/useMailClientHandlers'
import type { EmailActions } from '../hooks/useEmailActions'

/** Where an action can appear. A definition opts into surfaces; the resolver
 *  filters by the requesting surface. */
export type ActionSurface =
  | 'palette' // ⌘K list (searchable)
  | 'context-menu' // right-click on a message row
  | 'detail-header' // focused-mail action row (MessageHeader)
  | 'keyboard' // dispatchable via shortcut

/** Section ordering within menus / palette groups. Supersedes the three-value
 *  `ActionGroup` in `contextualActions.ts`. */
export type ActionSection =
  | 'open'
  | 'compose-reply'
  | 'state'
  | 'organize'
  | 'move'
  | 'navigate'
  | 'app'

/** Serializable shortcut descriptor — replaces the if-chains in dispatch.ts
 *  (consumed by the keyboard tier in a later slice). `key` is compared
 *  lowercased against `KeyboardEvent.key`. */
export interface ShortcutChord {
  key: string
  /** metaKey || ctrlKey (matches dispatch.ts). */
  mod?: boolean
  shift?: boolean
  alt?: boolean
  /** Fires even when an editable element is focused (the "modifier chords"
   *  tier). Default false. */
  inEditable?: boolean
}

/** A single action subject. `targets` is a list from day one so multi-select is
 *  a resolver/UI change later, not an every-action rewrite; today length ∈ {0,1}. */
export interface MessageTarget {
  ref: SourceMessageRef
  /** Summary when the surface has it (row, cached detail); label-flipping
   *  actions (toggle-read/flag) read it, falling back to the ref otherwise. */
  summary?: MessageSummary
  isDraft: boolean
  draftId?: string | null
  conversationId?: string
}

/** Everything the resolver knows at invocation time. Built fresh per event — a
 *  cheap plain object, no hooks inside. */
export interface ActionContext {
  /** The action's subject(s). For the context menu this is the right-clicked
   *  row; for keyboard/header/palette it is the focused selection. */
  targets: MessageTarget[]
  /** Role of the current view; null when ambiguous (search / unassigned smart
   *  mailbox). */
  viewRole: string | null
  activePane: PaneId
  surface: ActionSurface
  /** Overlay/surface ownership — global app actions stay available; message
   *  actions are suppressed while a surface owns the screen. */
  inputOwner: 'mail' | 'overlay' | 'surface'
  /** From `useEmailActions.isPending` — lets consumers render disabled/spinner. */
  hasPendingMutation: boolean
  /** Reserved: wire to daemon connection events later; 'unknown' ⇒ permissive. */
  connection: 'online' | 'offline' | 'unknown'
}

/** Injected once at registry-bind time (per provider render), NOT per action:
 *  the domain + app handler bundles that already exist. Slices add fields
 *  (navigation, overlays…) as surfaces migrate; Slice-1 definitions only touch
 *  `email`, so `app` is optional until Slice 2 threads it. */
export interface ActionServices {
  /** hooks/useEmailActions.ts — domain mutations (owns optimistic folds, toasts,
   *  undo). Actions delegate here; they never reimplement. */
  email: EmailActions
  /** app/useMailClientHandlers.ts — selection-scoped wrappers / navigation /
   *  overlays. Bound in later slices. */
  app?: ReturnType<typeof useMailClientHandlers>
}

/** Enablement result: `true` = runnable; `false` = shown-but-disabled with no
 *  hint; `{ reason }` = shown-but-disabled with hint text (palette
 *  discoverability). */
export type ActionEnablement = boolean | { reason: string }

export interface ActionDefinition {
  /** Stable namespaced id, e.g. `message.archive`. Persisted in
   *  recents/frequency counters later — never rename casually. */
  id: string
  section: ActionSection
  /** Static title, or derived from context (toggle-read/flag flip labels). */
  title: string | ((ctx: ActionContext) => string)
  icon: LucideIcon | ((ctx: ActionContext) => LucideIcon)
  /** Search terms for the palette. */
  keywords?: string
  surfaces: readonly ActionSurface[]
  shortcut?: ShortcutChord | readonly ShortcutChord[]
  /** Hidden entirely when false (context menu drops it; palette omits it). */
  isAvailable?: (ctx: ActionContext) => boolean
  /** Shown but not runnable when not `true`; `{ reason }` renders as hint. */
  isEnabled?: (ctx: ActionContext) => ActionEnablement
  destructive?: boolean
  /** Confirmation before run — routed through a shared dialog host (wired in a
   *  later slice). Slice-1 ports carry NONE, preserving today's behavior. */
  confirm?: { title: string; description: string; confirmLabel: string }
  /** Thin handler: delegates to {@link ActionServices}, which already own
   *  optimistic folds, toasts, and undo. */
  run: (ctx: ActionContext, services: ActionServices) => void | Promise<void>
}
