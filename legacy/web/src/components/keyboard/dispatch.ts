/**
 * Pure keyboard dispatch for the mail surface.
 *
 * One window listener (owned by {@link ./KeyboardController}) routes every key
 * through this function: modifier chords first, then mail-surface single keys,
 * pane rotation (`Shift+H`/`Shift+L` — `Tab` is left to native focus traversal
 * for accessibility), and finally the focused pane's own handler (`j`/`k`,
 * `h`/`l`). Keeping it pure makes the precedence order testable without a DOM.
 *
 * @spec docs/ui/L0#navigation-model
 * @spec docs/ui/L1#keyboard-shortcuts
 */
import { stepGotoPrefix, type GotoPrefix, type GotoRole } from './goto'
import { isEditableKeyboardTarget } from './inputTargets'

/** The keyboard-navigable regions of the mail shell. The detail pane is NOT
 *  focusable — it only displays the list's selected message, and `j`/`k` in the
 *  list drive it. */
export type PaneId = 'sidebar' | 'list'

/** Left-to-right pane order; drives `Shift+H`/`Shift+L` pane rotation. */
export const PANE_ORDER: readonly PaneId[] = ['sidebar', 'list']

/**
 * A pane's focused-key handler. Returns `true` when it consumed the event so
 * the dispatcher stops; `false` lets the key fall through to global actions.
 */
export type PaneKeyHandler = (event: KeyboardEvent) => boolean

/**
 * The registry tier of the dispatcher (PLAN-L2, Slice 5).
 *
 * The controller supplies a `match` built over the action registry: for a
 * pressed chord it resolves the `keyboard` surface in the CURRENT context and
 * returns a bound runner for the matching AVAILABLE action, or `null` to fall
 * through to native dispatch. The returned `run` already encapsulates the
 * destructive-confirm gate (it prompts before an irreversible delete), so the
 * dispatcher stays a thin, pure router.
 */
export interface RegistryKeyMatch {
  /** Resolved action id (for debugging / the ambiguity guard). */
  id: string
  /** Execute the action — instant, or after a confirm prompt for destructive
   *  actions. Owned by the controller so the dialog state lives in React-land. */
  run: () => void
}

export interface RegistryKeyHook {
  match(event: KeyboardEvent): RegistryKeyMatch | null
}

export interface KeyboardDispatchContext {
  /** A focused surface (settings/message/compose window) owns the screen. */
  effectiveSurfaceOpen: boolean
  /** A lightweight overlay (palette, compose, tag editor, shortcuts) owns input. */
  overlayOwnsInput: boolean
  hasSelectedMessage: boolean
  hasSearchQuery: boolean
  activePane: PaneId
  /** Panes currently mountable, in {@link PANE_ORDER}. */
  availablePanes: readonly PaneId[]
  focusPane: (pane: PaneId) => void
  /** Resolve the active pane's focused-key handler. */
  resolvePaneHandler: (pane: PaneId) => PaneKeyHandler | undefined
  /** Pending `g`/`gq` goto prefix, read synchronously at event time. */
  pendingPrefix: GotoPrefix
  setPendingPrefix: (prefix: GotoPrefix) => void
  onGoto: (role: GotoRole, options: { forceSmart: boolean }) => void
  /** `gc` — filter the list to the selected message's conversation. */
  onGotoConversation: () => void
  onOpenCommandPalette: () => void
  onOpenSettings: () => void
  onCompose: () => void
  onReply: () => void
  onReplyAll: () => void
  onToggleFlag: () => void
  onUndo: () => void
  onRedo: () => void
  onArchive: () => void
  onTrash: () => void
  onOpenTagEditor: () => void
  onOpenFocusedMessage: () => void
  onClearSelectedMessage: () => void
  onClearSearchQuery: () => void
  onToggleShortcuts: () => void
  /** Registry tier: contextual mail-action shortcuts (archive/trash/delete/tag).
   *  Consulted before the native selection-scoped handlers so the SAME chord
   *  (`#`/Backspace) resolves to move-to-trash outside Trash and
   *  delete-permanently inside it. Optional so `dispatchMailKey` stays usable in
   *  unit tests that only exercise native behaviors. */
  registryHook?: RegistryKeyHook
}

function moveFocus(ctx: KeyboardDispatchContext, direction: 1 | -1): void {
  const panes = ctx.availablePanes
  if (panes.length === 0) return
  const current = panes.indexOf(ctx.activePane)
  const base = current === -1 ? 0 : current
  const next = (base + direction + panes.length) % panes.length
  ctx.focusPane(panes[next])
}

/**
 * Route a keydown through the mail-surface keyboard map. Precedence:
 *  1. modifier chords (fire even inside text inputs, matching the legacy map);
 *  2. nothing else fires while typing;
 *  3. undo/redo, Escape clears, `?`/`/` (work regardless of overlay focus);
 *  4. pane-focus movement and the focused pane's handler;
 *  5. selection-scoped actions (`e`/`#`/`t`/`o`).
 */
export function dispatchMailKey(
  event: KeyboardEvent,
  ctx: KeyboardDispatchContext,
): void {
  // A focused surface renders the mail map inert (the daemon/surface owns keys).
  if (ctx.effectiveSurfaceOpen) return

  const mod = event.metaKey || event.ctrlKey
  const key = event.key
  const lower = key.toLowerCase()

  // ---- Modifier chords: intentionally fire even while a text input is focused. ----
  if (mod && lower === 'k') {
    event.preventDefault()
    ctx.onOpenCommandPalette()
    return
  }
  if (mod && key === ',') {
    event.preventDefault()
    ctx.onOpenSettings()
    return
  }
  if (mod && lower === 'n') {
    event.preventDefault()
    ctx.onCompose()
    return
  }
  if (mod && event.shiftKey && lower === 'r') {
    event.preventDefault()
    ctx.onReplyAll()
    return
  }
  if (mod && lower === 'r') {
    event.preventDefault()
    ctx.onReply()
    return
  }
  if (mod && event.shiftKey && lower === 'l') {
    event.preventDefault()
    if (ctx.hasSelectedMessage) ctx.onToggleFlag()
    return
  }

  if (isEditableKeyboardTarget(event.target)) return

  // ---- Undo/redo: only on the bare mail surface (native undo wins in overlays). ----
  if (mod && lower === 'z') {
    if (ctx.overlayOwnsInput) return
    event.preventDefault()
    if (event.shiftKey) {
      ctx.onRedo()
    } else {
      ctx.onUndo()
    }
    return
  }

  if (key === 'Escape' && !ctx.overlayOwnsInput) {
    if (ctx.hasSelectedMessage) {
      event.preventDefault()
      ctx.onClearSelectedMessage()
      return
    }
    if (ctx.hasSearchQuery) {
      event.preventDefault()
      ctx.onClearSearchQuery()
      return
    }
  }

  if (key === '?') {
    event.preventDefault()
    ctx.onToggleShortcuts()
    return
  }
  if (key === '/') {
    event.preventDefault()
    ctx.onOpenCommandPalette()
    return
  }

  // A pending goto prefix is cancelled the moment a text input takes focus
  // (e.g. the command palette opened via a chord above).
  if (ctx.pendingPrefix && isEditableKeyboardTarget(event.target)) {
    ctx.setPendingPrefix(null)
  }

  // ---- Plain keys below act on the bare mail surface only. ----
  if (ctx.overlayOwnsInput || event.altKey) return

  // ---- Goto prefix machine (`g`, then a role key, or `gq` + role). ----
  if (ctx.pendingPrefix) {
    const step = stepGotoPrefix(ctx.pendingPrefix, key)
    if (step.type === 'goto') {
      event.preventDefault()
      ctx.setPendingPrefix(null)
      ctx.onGoto(step.role, { forceSmart: step.forceSmart })
      return
    }
    if (step.type === 'goto-conversation') {
      event.preventDefault()
      ctx.setPendingPrefix(null)
      if (ctx.hasSelectedMessage) ctx.onGotoConversation()
      return
    }
    if (step.type === 'await-q') {
      event.preventDefault()
      ctx.setPendingPrefix('gq')
      return
    }
    // Unrecognized: drop the prefix and let this key be handled normally.
    ctx.setPendingPrefix(null)
  }
  if (lower === 'g' && !event.shiftKey) {
    event.preventDefault()
    ctx.setPendingPrefix('g')
    return
  }

  // Pane rotation: `Shift+H`/`Shift+L` (Vim-style, wraps at the ends). `Tab` is
  // deliberately NOT hijacked — it keeps its native focus-traversal behavior so
  // keyboard/AT users can still reach controls within a pane. Plain `h`/`l` fall
  // through to the focused pane's handler (the list collapses/expands threads).
  if (event.shiftKey && lower === 'h') {
    event.preventDefault()
    moveFocus(ctx, -1)
    return
  }
  if (event.shiftKey && lower === 'l') {
    event.preventDefault()
    moveFocus(ctx, 1)
    return
  }

  // Within-pane navigation (j/k vertical, h/l horizontal) belongs to the focused
  // pane. While a message is open the list stays the active pane, so j/k there
  // step the selection — which is what the detail pane shows.
  const paneHandler = ctx.resolvePaneHandler(ctx.activePane)
  if (paneHandler && paneHandler(event)) return

  // ---- Registry tier: contextual mail actions (archive/trash/delete/tag). ----
  // The resolver picks the AVAILABLE action for this chord IN THIS CONTEXT, so
  // `#`/Backspace lands on delete-permanently in Trash (with a confirm) and
  // move-to-trash elsewhere. A `null` match (no available action — e.g. a draft,
  // or no selection) falls through to the native handlers below, so every legacy
  // behavior is preserved.
  const registryMatch = ctx.registryHook?.match(event)
  if (registryMatch) {
    event.preventDefault()
    registryMatch.run()
    return
  }

  // Selection-scoped actions, available from any pane while a message is open.
  if (!ctx.hasSelectedMessage) return
  if (lower === 'e') {
    ctx.onArchive()
    return
  }
  if (key === '#' || key === 'Backspace') {
    ctx.onTrash()
    return
  }
  if (lower === 't') {
    event.preventDefault()
    ctx.onOpenTagEditor()
    return
  }
  if (lower === 'o') {
    event.preventDefault()
    ctx.onOpenFocusedMessage()
  }
}
