/**
 * Rows for the keyboard `ShortcutReference`.
 *
 * Action rows are derived from the registry — every definition that declares a
 * `shortcut` becomes a row, formatted from the very chord the keyboard tier
 * dispatches — so the reference cannot drift from what the keys actually do. The
 * native, non-action keys (pane navigation, the goto machine, command search)
 * that live in `dispatch.ts` rather than the registry are listed as a small
 * static set.
 *
 * Pure data → data; no DOM, so it is unit-testable.
 */
import { allActions, formatChords } from '@/commands'
import type { ActionContext } from '@/commands'

export interface ShortcutRow {
  keys: string[]
  action: string
}

/** Neutral context for resolving a definition's display title (toggle actions
 *  flip on the target; with no target they read their base label, e.g. "Flag"). */
const NEUTRAL_CTX: ActionContext = {
  targets: [],
  viewRole: null,
  activePane: 'list',
  surface: 'keyboard',
  inputOwner: 'mail',
  hasPendingMutation: false,
  connection: 'unknown',
}

/** The native keys handled directly by `dispatch.ts` (modes/navigation), which
 *  are deliberately NOT registry actions. Kept in sync by hand — a short, stable
 *  list that rarely changes. */
const NATIVE_SHORTCUTS: readonly ShortcutRow[] = [
  { keys: ['j', '↓'], action: 'Next conversation' },
  { keys: ['k', '↑'], action: 'Previous conversation' },
  { keys: ['h', '←'], action: 'Collapse conversation' },
  { keys: ['l', '→'], action: 'Expand conversation' },
  { keys: ['⇧ H'], action: 'Focus pane left' },
  { keys: ['⇧ L'], action: 'Focus pane right' },
  { keys: ['g i'], action: 'Go to inbox' },
  { keys: ['g a'], action: 'Go to archive' },
  { keys: ['g t'], action: 'Go to trash' },
  { keys: ['g c'], action: 'Go to conversation' },
  { keys: ['g q', '…'], action: 'Go to smart mailbox by role' },
  { keys: ['/'], action: 'Open command search' },
]

function displayTitle(
  title: string | ((ctx: ActionContext) => string),
): string {
  return typeof title === 'function' ? title(NEUTRAL_CTX) : title
}

/** Action rows derived from the registry, in registration (section) order. */
export function registryShortcutRows(): ShortcutRow[] {
  return allActions()
    .filter((def) => def.shortcut)
    .map((def) => ({
      keys: formatChords(def.shortcut),
      action: displayTitle(def.title),
    }))
}

/** The full reference: native navigation keys, then registry-derived actions. */
export function shortcutReferenceRows(): ShortcutRow[] {
  return [...NATIVE_SHORTCUTS, ...registryShortcutRows()]
}
