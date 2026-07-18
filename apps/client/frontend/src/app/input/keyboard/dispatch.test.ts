/**
 * Precedence tests for the pure mail-key dispatcher: the registry is the single
 * dispatch table, and the matched chord's tier flags place it in the order —
 * `aboveOverlay` chords fire above overlays (inside editables only with
 * `inEditable`), plain chords fire on the bare surface after the focused pane's
 * handler, and a chord the table does not claim does NOTHING (no native
 * fallback re-deciding availability).
 */
import { describe, expect, test } from 'bun:test'
import {
  dispatchMailKey,
  type KeyboardDispatchContext,
  type RegistryKeyMatch,
} from './dispatch'

interface StubEventInit {
  key: string
  metaKey?: boolean
  ctrlKey?: boolean
  shiftKey?: boolean
  altKey?: boolean
  /** `{ tagName: 'INPUT' }` simulates a focused text input (the editable
   *  predicate is duck-typed for exactly this DOM-free testability). */
  target?: { tagName?: string; isContentEditable?: boolean } | null
}

function stubEvent(init: StubEventInit) {
  const record = { prevented: false }
  const event = {
    key: init.key,
    metaKey: init.metaKey ?? false,
    ctrlKey: init.ctrlKey ?? false,
    shiftKey: init.shiftKey ?? false,
    altKey: init.altKey ?? false,
    target: init.target ?? null,
    preventDefault: () => {
      record.prevented = true
    },
  } as unknown as KeyboardEvent
  return { event, record }
}

function baseCtx(
  partial?: Partial<KeyboardDispatchContext>,
): KeyboardDispatchContext {
  return {
    effectiveSurfaceOpen: false,
    overlayOwnsInput: false,
    hasSelectedMessage: true,
    hasSearchQuery: false,
    activePane: 'list',
    availablePanes: ['sidebar', 'list'],
    focusPane: () => {},
    resolvePaneHandler: () => undefined,
    pendingPrefix: null,
    setPendingPrefix: () => {},
    onGoto: () => {},
    onGotoConversation: () => {},
    onOpenCommandPalette: () => {},
    onUndo: () => {},
    onRedo: () => {},
    onClearSelectedMessage: () => {},
    onClearSearchQuery: () => {},
    ...partial,
  }
}

function hookReturning(match: Omit<RegistryKeyMatch, 'run'> | null) {
  const runs: string[] = []
  const hook = {
    match: () =>
      match === null ? null : { ...match, run: () => runs.push(match.id) },
  }
  return { hook, runs }
}

describe('dispatchMailKey — registry tiers', () => {
  test('an aboveOverlay chord fires above a lightweight overlay', () => {
    const { hook, runs } = hookReturning({
      id: 'app.shortcuts',
      aboveOverlay: true,
      inEditable: false,
    })
    const { event, record } = stubEvent({ key: '?', shiftKey: true })
    dispatchMailKey(event, baseCtx({ overlayOwnsInput: true, registryHook: hook }))
    expect(runs).toEqual(['app.shortcuts'])
    expect(record.prevented).toBe(true)
  })

  test('inside an editable, an aboveOverlay chord fires only with inEditable', () => {
    const editable = { tagName: 'INPUT' }
    const modChord = {
      id: 'app.compose',
      aboveOverlay: true,
      inEditable: true,
    }
    const fired = hookReturning(modChord)
    dispatchMailKey(
      stubEvent({ key: 'n', metaKey: true, target: editable }).event,
      baseCtx({ registryHook: fired.hook }),
    )
    expect(fired.runs).toEqual(['app.compose'])

    const held = hookReturning({ ...modChord, inEditable: false })
    dispatchMailKey(
      stubEvent({ key: 'n', metaKey: true, target: editable }).event,
      baseCtx({ registryHook: held.hook }),
    )
    expect(held.runs).toEqual([])
  })

  test('a plain mail-action chord stays on the bare surface, after the pane handler', () => {
    const plain = {
      id: 'message.archive',
      aboveOverlay: false,
      inEditable: false,
    }
    // Overlay owns input: the plain tier never fires.
    const overlaid = hookReturning(plain)
    dispatchMailKey(
      stubEvent({ key: 'e' }).event,
      baseCtx({ overlayOwnsInput: true, registryHook: overlaid.hook }),
    )
    expect(overlaid.runs).toEqual([])
    // The focused pane's handler wins over the plain tier.
    const paneClaims = hookReturning(plain)
    dispatchMailKey(
      stubEvent({ key: 'e' }).event,
      baseCtx({
        registryHook: paneClaims.hook,
        resolvePaneHandler: () => () => true,
      }),
    )
    expect(paneClaims.runs).toEqual([])
    // Bare surface, unclaimed by the pane: the table's action runs.
    const bare = hookReturning(plain)
    const { event, record } = stubEvent({ key: 'e' })
    dispatchMailKey(event, baseCtx({ registryHook: bare.hook }))
    expect(bare.runs).toEqual(['message.archive'])
    expect(record.prevented).toBe(true)
  })

  test('a chord the table does not claim does nothing — no native fallback', () => {
    // `e` in the Archive/Trash view resolves to null (the table hides archive
    // there); the legacy fallback used to archive anyway. Now: inert.
    const { hook } = hookReturning(null)
    const { event, record } = stubEvent({ key: 'e' })
    dispatchMailKey(event, baseCtx({ registryHook: hook }))
    expect(record.prevented).toBe(false)
  })

  test('a focused surface renders every tier inert', () => {
    const { hook, runs } = hookReturning({
      id: 'app.compose',
      aboveOverlay: true,
      inEditable: true,
    })
    dispatchMailKey(
      stubEvent({ key: 'n', metaKey: true }).event,
      baseCtx({ effectiveSurfaceOpen: true, registryHook: hook }),
    )
    expect(runs).toEqual([])
  })
})
