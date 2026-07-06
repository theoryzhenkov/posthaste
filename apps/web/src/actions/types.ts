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
import type { Mailbox, MessageSummary, SourceMessageRef } from '../api/types'
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

/**
 * One choosable target of a PARAMETERIZED action (e.g. a mailbox for
 * `message.move-to-mailbox`, a snooze preset for `message.snooze`). Pure data —
 * `id` is the value `run` receives, `label` is what every surface renders
 * (context submenu row, palette pick-step row, header popover row).
 */
export interface ActionParamOption {
  id: string
  label: string
  icon?: LucideIcon
  /** Extra search terms for the palette's pick-step filter. */
  keywords?: string
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
  /** Row-scoped navigation the context menu binds per message row: the two
   *  `open` entries delegate here. Absent on every non-row surface — which is
   *  exactly how those entries stay context-menu-only (their `isAvailable`
   *  gates on `row` being bound). */
  row?: {
    open: (message: MessageSummary) => void
    viewConversation: (message: MessageSummary) => void
  }
  /** The account-scoped mailbox read model (the same source the sidebar /
   *  navigation read models hydrate). Bound by surfaces that can offer
   *  mailbox-parameterized actions (context menu via `useMailboxDirectory`,
   *  palette + keyboard via the navigation read models); absent elsewhere —
   *  which is exactly how `move-to-mailbox` stays hidden where no mailbox
   *  source exists (e.g. the email-only parity harness). */
  mailboxes?: {
    list: (sourceId: string) => Mailbox[]
  }
  /** Detail-header-scoped handler bindings, bound by `MessageHeader` from its
   *  callback props. Lets both header hosts (the mail shell, which also has
   *  `app`, and the focused message window, which does not) drive the same
   *  definitions. Defs prefer these over `app` when present. */
  detail?: {
    reply: () => void
    replyAll: () => void
    forward: () => void
    editDraft?: () => void
    openTagEditor?: () => void
    openFocusedMessage?: () => void
  }
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
  /** Hidden entirely when false (context menu drops it; palette omits it).
   *  Receives the bound `services` too, so an action can gate on a capability
   *  the surface provides (e.g. the row-scoped `open` entries require
   *  `services.row`). Most predicates only read `ctx`. */
  isAvailable?: (ctx: ActionContext, services: ActionServices) => boolean
  /** Shown but not runnable when not `true`; `{ reason }` renders as hint. */
  isEnabled?: (ctx: ActionContext) => ActionEnablement
  destructive?: boolean
  /** Confirmation before run — routed through a shared dialog host (wired in a
   *  later slice). Slice-1 ports carry NONE, preserving today's behavior. */
  confirm?: { title: string; description: string; confirmLabel: string }
  /**
   * PARAMETERIZED actions: present iff the action needs a user-chosen target
   * (a mailbox, a snooze preset) before it can run. Returns the choosable
   * options for this context — the resolver exposes them as
   * `ResolvedAction.params`, and each surface renders its own picker (context
   * submenu, palette pick-step, header popover; a keyboard chord opens the
   * palette picker). An action whose options resolve empty is dropped like a
   * failed `isAvailable`. Non-parameterized actions simply omit this.
   */
  resolveParams?: (
    ctx: ActionContext,
    services: ActionServices,
  ) => ActionParamOption[]
  /** Thin handler: delegates to {@link ActionServices}, which already own
   *  optimistic folds, toasts, and undo. A parameterized action receives the
   *  chosen `param` (and must no-op without one — surfaces always supply it). */
  run: (
    ctx: ActionContext,
    services: ActionServices,
    param?: ActionParamOption,
  ) => void | Promise<void>
}
