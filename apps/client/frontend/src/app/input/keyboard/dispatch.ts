/**
 * Pure keyboard dispatch for the mail surface.
 *
 * One window listener (owned by {@link ./KeyboardController}) routes every key
 * through this function. The action registry is the single dispatch table: the
 * controller supplies a `match` resolved over it, and the matched chord's tier
 * flags decide WHERE in the precedence order it fires (see
 * {@link dispatchMailKey}). The only native keys are the ones with no registry
 * definition at all: the palette openers (⌘K, `/` — the opener clears the
 * palette's seeded pick-step, a shell concern), undo/redo (⌘Z/⌘⇧Z), Escape
 * clears, the goto prefix machine, reading-pane paging (Space/Shift+Space —
 * the detail pane is display-only, so its scrolling cannot be a pane
 * handler), and pane movement (`Shift+H`/`Shift+L` —
 * `Tab` is left to native focus traversal for accessibility). Keeping the
 * function pure makes the precedence order testable without a DOM.
 */
import { PANE_ID, type PaneId } from '@/domain/vocabulary'
import type { PaneKeyHandler } from '@/components/keyboard/context'
import { stepGotoPrefix, type GotoPrefix, type GotoRole } from '../goto/goto'
import { isEditableKeyboardTarget } from '@/lib/dom'

/** Left-to-right pane order; drives `Shift+H`/`Shift+L` pane rotation. */
export const PANE_ORDER: readonly PaneId[] = [PANE_ID.Sidebar, PANE_ID.List]

export type { PaneKeyHandler }

/**
 * The registry tier of the dispatcher.
 *
 * The controller supplies a `match` built over the action registry: for a
 * pressed chord it resolves the `keyboard` surface in the current context and
 * returns a bound runner for the matching action, or `null` when no definition
 * claims the chord here. The returned `run` already encapsulates the
 * destructive-confirm gate (it prompts before an irreversible delete) and the
 * disabled-swallow (a claimed chord with nothing to act on runs as a no-op —
 * so ⌘R with no selection still never reaches the browser), keeping the
 * dispatcher a thin, pure router.
 */
export interface RegistryKeyMatch {
  /** Resolved action id (for debugging / the ambiguity guard). */
  id: string
  /** The matched chord fires above lightweight overlays (the modifier-chord
   *  tier) instead of only on the bare mail surface. */
  aboveOverlay: boolean
  /** The matched chord fires even while an editable element is focused. */
  inEditable: boolean
  /** Execute the action — instant, or after a confirm prompt for destructive
   *  actions. Owned by the controller so the dialog state lives in React-land. */
  run: () => void
}

interface RegistryKeyHook {
  match(event: KeyboardEvent): RegistryKeyMatch | null
}

export interface KeyboardDispatchContext {
  /** A focused surface (settings/message/compose window) owns the screen. */
  effectiveSurfaceOpen: boolean
  /** A lightweight overlay (palette, compose, tag editor, shortcuts) owns input. */
  overlayOwnsInput: boolean
  hasSelectedMessage: boolean
  /** A message is OPEN in the reader (Escape closes it, keeping the cursor). */
  hasOpenedMessage: boolean
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
  /** Space / Shift+Space — page the reading pane's scroll container. Returns
   *  `false` when there is nothing to scroll so the key falls through. */
  onScrollMessagePane: (direction: 1 | -1) => boolean
  onOpenCommandPalette: () => void
  onUndo: () => void
  onRedo: () => void
  onCloseReader: () => void
  onClearSearchQuery: () => void
  /** Registry tier: every action chord — modifier chords (⌘R/⌘⇧R/⌘N/⌘,/⌘⇧L/`?`)
   *  and the contextual mail actions (archive/trash/delete/tag/open). The
   *  matched chord's flags place it in the precedence order; availability is
   *  the table's alone (no native fallback re-deciding it). Optional so
   *  `dispatchMailKey` stays usable in unit tests that only exercise native
   *  behaviors. */
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
 *  1. the native palette chord (⌘K) and the registry's `aboveOverlay` chords
 *     (fire above overlays; inside text inputs only when `inEditable`);
 *  2. nothing else fires while typing;
 *  3. undo/redo, Escape clears, `/` (work regardless of overlay focus);
 *  4. reading-pane paging (Space/Shift+Space), pane-focus movement, and the
 *     focused pane's handler;
 *  5. the registry's plain mail-action chords (`e`/`#`/`u`/`m`/`t`/`o`), whose
 *     availability the action table alone decides — a chord no definition
 *     claims in this context does nothing (e.g. `e` in the Archive view).
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
  const editable = isEditableKeyboardTarget(event.target)

  // ⌘K: native palette opener (no registry definition — see module header).
  // Intentionally fires even while a text input is focused.
  if (mod && lower === 'k') {
    event.preventDefault()
    ctx.onOpenCommandPalette()
    return
  }

  // ONE registry resolution per keydown; the matched chord's flags pick the tier.
  const match = ctx.registryHook?.match(event) ?? null

  // ---- Registry chord tier: fires above overlays; inside a text input only
  // when the chord declares `inEditable` (the modifier chords do; `?` doesn't).
  if (match?.aboveOverlay && (match.inEditable || !editable)) {
    event.preventDefault()
    match.run()
    return
  }

  if (editable) return

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
    if (ctx.hasOpenedMessage) {
      event.preventDefault()
      ctx.onCloseReader()
      return
    }
    if (ctx.hasSearchQuery) {
      event.preventDefault()
      ctx.onClearSearchQuery()
      return
    }
  }

  if (key === '/') {
    event.preventDefault()
    ctx.onOpenCommandPalette()
    return
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

  // ---- Reading-pane paging: Space / Shift+Space scroll the open message.
  // The detail pane is display-only (never a focus region), so this is a
  // native behavior, not a pane handler. Falls through when nothing scrolls.
  if (key === ' ' && ctx.hasSelectedMessage) {
    if (ctx.onScrollMessagePane(event.shiftKey ? -1 : 1)) {
      event.preventDefault()
      return
    }
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
  // pane. While a message is open the list stays the active pane; j/k there
  // steps the SELECTION cursor while the detail pane keeps the OPENED message
  // (Enter re-opens the cursor row, realigning the two).
  const paneHandler = ctx.resolvePaneHandler(ctx.activePane)
  if (paneHandler && paneHandler(event)) return

  // ---- Registry mail-action tier: contextual chords (archive/trash/delete/
  // tag/open). The resolver picks the action for this chord IN THIS CONTEXT, so
  // `#`/Backspace lands on delete-permanently in Trash (with a confirm),
  // discard-draft on a draft, and move-to-trash elsewhere. The table's verdict
  // is final: a `null` match means the chord does nothing here — there is no
  // native fallback second-guessing availability.
  if (match) {
    event.preventDefault()
    match.run()
  }
}
