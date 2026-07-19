/**
 * Unified action registry — core types.
 *
 * ONE definition per action, from which every surface (palette, context menu,
 * detail header, keyboard) resolves. Definitions are pure data: titles/icons
 * are values or `(ctx) => value` functions, never JSX, so the whole registry is
 * unit-testable without a DOM.
 *
 * The domain-free half of this contract (sections, param options, confirm
 * copy, the resolved view components render, scope services) lives in
 * `lib/command.ts` so `components/` can consume resolved actions without ever
 * importing `commands/` (R11: commands bind UI to verbs, never the reverse).
 */
import type { LucideIcon } from 'lucide-react'
import type { Mailbox, MessageSummary, SourceMessageRef } from '../data/transport/api/index'
import type { PaneId } from '../domain/vocabulary'
import type {
  ActionConfirmCopy,
  ActionParamOption,
  ActionSection,
  CommandInputOwner,
  CommandScopeServices,
} from '../lib/command'
import type { EmailActions } from '../data/hooks/useEmailActions'

export type { ActionConfirmCopy, ActionParamOption, ActionSection }

/** Where an action can appear. A definition opts into surfaces; the resolver
 *  filters by the requesting surface. */
type ActionSurface =
  | 'palette' // ⌘K list (searchable)
  | 'context-menu' // right-click on a message row
  | 'detail-header' // focused-mail action row (MessageHeader)
  | 'keyboard' // dispatchable via shortcut

/** Serializable shortcut descriptor. `key` is compared lowercased against
 *  `KeyboardEvent.key`; a `code` chord matches `KeyboardEvent.code` instead
 *  (layout-independent — e.g. macOS ⌥ dead keys mangle `key`). */
export interface ShortcutChord {
  key: string
  /** Match on `event.code` (physical key) instead of `event.key`; `key` then
   *  only names the chord for display. */
  code?: string
  /** metaKey || ctrlKey. */
  mod?: boolean
  shift?: boolean
  alt?: boolean
  /** Fires even when an editable element is focused (the "modifier chords"
   *  tier). Default false. */
  inEditable?: boolean
  /** Mail-dispatch tier: fires above lightweight overlays (palette, compose,
   *  tag editor, shortcuts reference) — the former hard-coded modifier-chord
   *  tier of `dispatchMailKey`. Unset = a plain mail-action chord that only
   *  fires on the bare surface, after the focused pane's handler. */
  aboveOverlay?: boolean
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
  inputOwner: CommandInputOwner
  /** From `useEmailActions.isPending` — lets consumers render disabled/spinner. */
  hasPendingMutation: boolean
  /** Reserved: wire to daemon connection events later; 'unknown' ⇒ permissive. */
  connection: 'online' | 'offline' | 'unknown'
}

/** The app-level handler bundle actions delegate to — the slice of
 *  `useMailClientHandlers` the definitions consume. Declared HERE (not
 *  inferred from `app/`) so `commands/` never imports the composition root
 *  (R11); the app-side bundle satisfies it structurally. */
interface MailAppCommandHandlers {
  handleCompose: () => void
  handleOpenSettings: (
    category?: 'accounts' | 'mailboxes' | 'tags' | 'general' | 'appearance',
  ) => void
  handleToggleShortcuts: () => void
  handleSelectMessage: (message: MessageSummary) => void
  handleSearch: (query: string, append?: boolean) => void
  handleOpenFocusedMessage: () => void
  handleReply: () => void
  handleReplyAll: () => void
  handleForward: () => void
  handleEditDraft: () => void
  handleOpenTagEditor: () => void
}

/** Injected once at registry-bind time (per provider render), NOT per action:
 *  the domain + app handler bundles that already exist. Extends the
 *  domain-free scope services (`desktop`, `surfaceHost`, `compose`) the
 *  dispatcher's scopes bind. */
export interface ActionServices extends CommandScopeServices {
  /** hooks/useEmailActions.ts — domain mutations (owns optimistic folds, toasts,
   *  undo). Actions delegate here; they never reimplement. Absent in scopes
   *  without mail context (the dispatcher's global/overlay scopes), where no
   *  message action can resolve anyway (no targets). */
  email?: EmailActions
  /** The mail shell's handler bundle (`useMailClientHandlers`) — selection-
   *  scoped wrappers / navigation / overlays. */
  app?: MailAppCommandHandlers
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
  /** Detail-header-scoped handler bindings, bound by the header's host from
   *  its callbacks. Lets both header hosts (the mail shell, which also has
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
  /** The three `message.unsubscribe` execution paths, bound ONLY by hosts whose
   *  execution route honors the destructive `confirm` gate (the detail header
   *  today) — the one-click POST must never run without its confirm dialog.
   *  Absent binding hides the action (capability gating, like `row`). */
  unsubscribe?: {
    /** Confirmed RFC 8058 one-click: the backend performs the POST; the
     *  implementation owns success/failure toasts (useEmailActions). */
    oneClick: (ref: SourceMessageRef) => void | Promise<void>
    /** Open the composer prefilled from the `mailto:` URI — the user sends. */
    mailto: (mailtoUri: string) => void
    /** Open the plain (non-one-click) https target in the system browser. */
    openLink: (url: string) => void | Promise<void>
  }
}


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
  /** Not runnable (and not rendered by any surface) when `false`; the keyboard
   *  dispatcher still lets a disabled action claim its chord so the event is
   *  swallowed rather than falling through to the browser. */
  isEnabled?: (ctx: ActionContext) => boolean
  destructive?: boolean
  /** Confirmation before run — routed through a shared dialog host. Either
   *  static copy, or derived from context (`message.unsubscribe` confirms only
   *  its one-click path and names the sender); a function returning
   *  `undefined` means "no confirmation for this context". */
  confirm?:
    | ActionConfirmCopy
    | ((ctx: ActionContext) => ActionConfirmCopy | undefined)
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
