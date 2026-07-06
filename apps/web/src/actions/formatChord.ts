/**
 * Human-readable rendering of a {@link ShortcutChord} (PLAN-L2, Slice 3).
 *
 * One formatter feeds the palette row shortcut hints today and (Slice 5) the
 * generated `ShortcutReference` and button titles — so the chord a definition
 * declares is the single source of truth for what the user is told to press.
 *
 * Pure data → string; no DOM, unit-testable.
 *
 * @spec docs/eph/PLAN-L2-action-registry.md
 */
import type { ShortcutChord } from './types'

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
  return `${chord.mod ? MOD : ''}${chord.shift ? SHIFT : ''}${
    chord.alt ? ALT : ''
  }${formatKey(chord.key)}`
}
