/**
 * Keyboard ↔ registry bridge.
 *
 * The keyboard tier of `dispatchMailKey` no longer hard-codes mail-action
 * handlers: for a pressed chord it builds a `surface: 'keyboard'`
 * {@link ActionContext} and asks the SAME resolver every other surface uses to
 * pick the matching AVAILABLE action. Because the resolver filters by
 * `isAvailable(ctx)`, one chord (`#`/Backspace) resolves to `move-to-trash`
 * outside Trash and `delete-permanently` inside it — the contextual fix.
 *
 * Pure data → data (chord vs `KeyboardEvent`, resolved action out); no DOM, so
 * it is unit-testable exactly like the mail-key dispatcher.
 *
 */
import type {
  ActionConfirmCopy,
  ActionContext,
  ActionServices,
  ShortcutChord,
} from './types'
import { runActionWithConfirm } from '../lib/command'
import { resolveActions, type ResolvedAction } from './resolve'

/** A `KeyboardEvent`-shaped subset — the only fields chord matching reads. Keeps
 *  the matcher testable with a plain object (no synthetic DOM events). */
export interface ChordEvent {
  key: string
  /** Physical key (`KeyboardEvent.code`); only read by `code`-declared chords. */
  code?: string
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
 * match even though `event.shiftKey` is true. `key` is compared lowercased; a
 * chord declaring `code` compares `event.code` instead (layout-independent —
 * macOS ⌥ chords turn `key` into dead/accented characters).
 */
export function matchesChord(chord: ShortcutChord, event: ChordEvent): boolean {
  const mod = event.metaKey || event.ctrlKey
  if ((chord.mod ?? false) !== mod) return false
  if ((chord.alt ?? false) !== event.altKey) return false
  if (chord.shift !== undefined && chord.shift !== event.shiftKey) return false
  if (chord.code !== undefined) return chord.code === event.code
  return chord.key.toLowerCase() === event.key.toLowerCase()
}

/** Does ANY of a definition's chord(s) match this event? */
export function shortcutMatches(
  shortcut: ShortcutChord | readonly ShortcutChord[] | undefined,
  event: ChordEvent,
): boolean {
  return firstMatchingChord(shortcut, event) !== null
}

/** The chord of a definition that matched this event, or `null`. The mail
 *  dispatcher reads the matched chord's tier flags (`inEditable`,
 *  `aboveOverlay`) to place the action in its precedence order. */
export function firstMatchingChord(
  shortcut: ShortcutChord | readonly ShortcutChord[] | undefined,
  event: ChordEvent,
): ShortcutChord | null {
  if (!shortcut) return null
  const chords = Array.isArray(shortcut)
    ? shortcut
    : [shortcut as ShortcutChord]
  return chords.find((chord) => matchesChord(chord, event)) ?? null
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
 *
 * `includeDisabled` keeps shown-but-disabled matches (the mail dispatcher passes
 * it so a claimed chord — e.g. ⌘R with nothing selected — still swallows the
 * event instead of leaking to the browser default; the caller must not run a
 * disabled resolution).
 */
export function resolveKeyboardAction(
  event: ChordEvent,
  ctx: ActionContext,
  services: ActionServices,
  opts?: { includeDisabled?: boolean },
): ResolvedAction | null {
  const matches = resolveActions(ctx, services, opts).filter((r) =>
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

/** Confirmation copy carried by a destructive action (context-resolved — see
 *  `ResolvedAction.confirm`). */
export type ActionConfirm = ActionConfirmCopy

/**
 * Run a resolved action, honoring its destructive `confirm` gate — the
 * registry-typed face of `lib/command.runActionWithConfirm` (one gate, every
 * surface).
 *
 * Non-destructive actions (move-to-trash, archive, tag) run instantly — matching
 * today's keyboard feel. A `confirm`-bearing action (delete-permanently) is
 * NEVER executed straight from a keystroke: it routes through `requestConfirm`,
 * and only runs if the user accepts. This is what stops `#`/Backspace in Trash
 * from silently, irreversibly destroying a message.
 *
 * A PARAMETERIZED action (`def.resolveParams`) cannot run from a bare chord —
 * there is no chosen target yet. It routes through `requestParam` (the
 * controller opens the command palette in that action's pick-step) instead of
 * silently no-oping; without a `requestParam` host it is skipped entirely.
 */
export function runResolvedWithConfirm(
  resolved: ResolvedAction,
  requestConfirm: (confirm: ActionConfirm, onConfirm: () => void) => void,
  requestParam?: (resolved: ResolvedAction) => void,
): void {
  runActionWithConfirm(
    resolved,
    requestConfirm,
    requestParam ? () => requestParam(resolved) : undefined,
  )
}

// ---------------------------------------------------------------------------
// Chord formatting.
//
// One formatter feeds the palette row shortcut hints, the generated
// `ShortcutReference`, and button titles — so the chord a definition declares
// is the single source of truth for what the user is told to press.
// ---------------------------------------------------------------------------

const MOD = '⌘' // ⌘
const SHIFT = '⇧' // ⇧
const ALT = '⌥' // ⌥

/** Named keys that read better than a bare `KeyboardEvent.key`. */
const KEY_LABELS: Record<string, string> = {
  backspace: 'Backspace',
  enter: 'Enter',
  escape: 'Esc',
  arrowup: '↑',
  arrowdown: '↓',
  arrowleft: '←',
  arrowright: '→',
  ' ': 'Space',
}

function formatKey(key: string): string {
  const lower = key.toLowerCase()
  if (KEY_LABELS[lower]) return KEY_LABELS[lower]
  // Single letters read best uppercased ("E", "⌘R"); symbols pass through.
  return key.length === 1 ? key.toUpperCase() : key
}

/**
 * Render the primary chord as a compact string (e.g. `⌘⇧L`, `E`, `#`). Chord
 * arrays render their FIRST entry — the canonical binding a hint should teach.
 * Returns `undefined` when there is no shortcut, so callers can omit the hint.
 */
export function formatChord(
  shortcut?: ShortcutChord | readonly ShortcutChord[],
): string | undefined {
  if (!shortcut) return undefined
  const chord = Array.isArray(shortcut)
    ? shortcut[0]
    : (shortcut as ShortcutChord)
  if (!chord) return undefined
  return formatSingle(chord)
}

function formatSingle(chord: ShortcutChord): string {
  return `${chord.mod ? MOD : ''}${chord.shift ? SHIFT : ''}${
    chord.alt ? ALT : ''
  }${formatKey(chord.key)}`
}

/**
 * Render EVERY chord a definition declares (e.g. `['#', 'Backspace']`) — used by
 * the generated `ShortcutReference`, which lists each alternative key. Empty when
 * there is no shortcut.
 */
export function formatChords(
  shortcut?: ShortcutChord | readonly ShortcutChord[],
): string[] {
  if (!shortcut) return []
  const chords = Array.isArray(shortcut)
    ? shortcut
    : [shortcut as ShortcutChord]
  return chords.map(formatSingle)
}
