/**
 * Keyboard ↔ registry bridge (PLAN-L2, Slice 5).
 *
 * The keyboard tier of `dispatchMailKey` no longer hard-codes mail-action
 * handlers: for a pressed chord it builds a `surface: 'keyboard'`
 * {@link ActionContext} and asks the SAME resolver every other surface uses to
 * pick the matching AVAILABLE action. Because the resolver filters by
 * `isAvailable(ctx)`, one chord (`#`/Backspace) resolves to `move-to-trash`
 * outside Trash and `delete-permanently` inside it — the contextual fix.
 *
 * Pure data → data (chord vs `KeyboardEvent`, resolved action out); no DOM, so
 * it is unit-testable exactly like `components/keyboard/dispatch.ts`.
 *
 * @spec docs/eph/PLAN-L2-action-registry.md
 */
import type { ActionContext, ActionServices, ShortcutChord } from './types'
import { resolveActions, type ResolvedAction } from './resolve'
import type { ActionDefinition } from './types'

/** A `KeyboardEvent`-shaped subset — the only fields chord matching reads. Keeps
 *  the matcher testable with a plain object (no synthetic DOM events). */
export interface ChordEvent {
  key: string
  metaKey: boolean
  ctrlKey: boolean
  shiftKey: boolean
  altKey: boolean
}

/**
 * Does a single chord match this key event?
 *
 * `mod` (⌘/Ctrl) and `alt` are strict (an unset flag must be absent). `shift`
 * is only enforced when the chord declares it — a shifted symbol like `#`
 * already encodes Shift in `event.key`, so a bare `{ key: '#' }` must still
 * match even though `event.shiftKey` is true. `key` is compared lowercased.
 */
export function matchesChord(chord: ShortcutChord, event: ChordEvent): boolean {
  const mod = event.metaKey || event.ctrlKey
  if ((chord.mod ?? false) !== mod) return false
  if ((chord.alt ?? false) !== event.altKey) return false
  if (chord.shift !== undefined && chord.shift !== event.shiftKey) return false
  return chord.key.toLowerCase() === event.key.toLowerCase()
}

/** Does ANY of a definition's chord(s) match this event? */
export function shortcutMatches(
  shortcut: ShortcutChord | readonly ShortcutChord[] | undefined,
  event: ChordEvent,
): boolean {
  if (!shortcut) return false
  const chords = Array.isArray(shortcut)
    ? shortcut
    : [shortcut as ShortcutChord]
  return chords.some((chord) => matchesChord(chord, event))
}

/**
 * Resolve the registry action bound to a pressed chord in this keyboard context.
 *
 * Resolves the `keyboard` surface (availability + enablement already applied),
 * then keeps only definitions whose shortcut matches the event. The resolver's
 * availability filter is what makes `#` context-sensitive; enablement drops
 * no-target actions so a bare keystroke never fires against nothing.
 *
 * Returns the single match, or `null` to fall through to native dispatch. When
 * more than one AVAILABLE action claims the same chord that is a definition bug
 * (overlapping `isAvailable` predicates) — flagged loudly in dev via
 * {@link assertUniqueChord}; the first is returned so production stays usable.
 */
export function resolveKeyboardAction(
  event: ChordEvent,
  ctx: ActionContext,
  services: ActionServices,
): ResolvedAction | null {
  const matches = resolveActions(ctx, services).filter((r) =>
    shortcutMatches(r.def.shortcut, event),
  )
  if (matches.length > 1) assertUniqueChord(event, matches)
  return matches[0] ?? null
}

/** Dev-mode guard: at most one AVAILABLE keyboard action may claim a chord in a
 *  given context (the "one-chord-two-actions disambiguated by availability"
 *  invariant). Overlapping availability predicates are a definition bug. */
function assertUniqueChord(event: ChordEvent, matches: ResolvedAction[]): void {
  const message = `ambiguous keyboard chord "${event.key}" resolves to multiple available actions: ${matches
    .map((m) => m.def.id)
    .join(', ')}`
  // Vite injects import.meta.env; guard for the bun test runner where it may be
  // absent. Throwing in dev turns an overlap into an immediate, loud failure.
  if (import.meta.env?.DEV) throw new Error(message)
  console.error(message)
}

/** Confirmation copy carried by a destructive action. */
export type ActionConfirm = NonNullable<ActionDefinition['confirm']>

/**
 * Run a resolved action, honoring its destructive `confirm` gate.
 *
 * Non-destructive actions (move-to-trash, archive, tag) run instantly — matching
 * today's keyboard feel. A `confirm`-bearing action (delete-permanently) is
 * NEVER executed straight from a keystroke: it routes through `requestConfirm`,
 * and only runs if the user accepts. This is what stops `#`/Backspace in Trash
 * from silently, irreversibly destroying a message.
 */
export function runResolvedWithConfirm(
  resolved: ResolvedAction,
  requestConfirm: (confirm: ActionConfirm, onConfirm: () => void) => void,
): void {
  const confirm = resolved.def.confirm
  if (confirm) {
    requestConfirm(confirm, () => void resolved.execute())
    return
  }
  void resolved.execute()
}
