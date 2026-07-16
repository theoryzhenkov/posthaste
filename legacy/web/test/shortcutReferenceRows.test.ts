/**
 * PLAN-L2 Slice 5 — the shortcut reference is GENERATED from the registry.
 *
 * The action rows must reflect the definitions' chords (so the help overlay can
 * never drift from what the keys actually do), while the native, non-action keys
 * stay listed. Asserting against the registry proves the derivation, not a
 * hand-copied snapshot.
 */
import { describe, expect, it } from 'bun:test'

import {
  registryShortcutRows,
  shortcutReferenceRows,
} from '../src/components/shortcutReferenceRows'
import { allActions } from '../src/actions'

describe('shortcutReferenceRows', () => {
  // spec: docs/eph/PLAN-L2-action-registry.md
  it('derives one row per registered action that declares a shortcut', () => {
    const withShortcut = allActions().filter((a) => a.shortcut)
    expect(registryShortcutRows()).toHaveLength(withShortcut.length)
  })

  it('renders the archive `E` and the #/Backspace trash chords from the defs', () => {
    const rows = registryShortcutRows()
    const archive = rows.find((r) => r.action === 'Archive')
    expect(archive?.keys).toEqual(['E'])
    const trash = rows.find((r) => r.action === 'Move to Trash')
    expect(trash?.keys).toEqual(['#', 'Backspace'])
    // The contextual sibling is listed too (same keys, different action).
    const del = rows.find((r) => r.action === 'Delete permanently')
    expect(del?.keys).toEqual(['#', 'Backspace'])
  })

  it('resolves function titles (toggle-flag → base label) with a neutral context', () => {
    const flag = registryShortcutRows().find((r) => r.action === 'Flag')
    expect(flag?.keys).toEqual(['⌘⇧L'])
  })

  it('keeps the native navigation keys (j/k, panes, goto, command search)', () => {
    const actions = shortcutReferenceRows().map((r) => r.action)
    expect(actions).toContain('Next conversation')
    expect(actions).toContain('Focus pane right')
    expect(actions).toContain('Go to inbox')
    expect(actions).toContain('Open command search')
  })
})
