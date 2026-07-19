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
    hasOpenedMessage: true,
    hasSearchQuery: false,
    activePane: 'list',
    availablePanes: ['sidebar', 'list'],
    focusPane: () => {},
    resolvePaneHandler: () => undefined,
    pendingPrefix: null,
    setPendingPrefix: () => {},
    onGoto: () => {},
    onGotoConversation: () => {},
    onScrollMessagePane: () => false,
    onOpenCommandPalette: () => {},
    onUndo: () => {},
    onRedo: () => {},
    onCloseReader: () => {},
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

  test('Space / Shift+Space page the reading pane while a message is open', () => {
    const scrolled: number[] = []
    const scrollingCtx = () =>
      baseCtx({
        onScrollMessagePane: (direction) => {
          scrolled.push(direction)
          return true
        },
      })
    const down = stubEvent({ key: ' ' })
    dispatchMailKey(down.event, scrollingCtx())
    const up = stubEvent({ key: ' ', shiftKey: true })
    dispatchMailKey(up.event, scrollingCtx())
    expect(scrolled).toEqual([1, -1])
    expect(down.record.prevented).toBe(true)
    expect(up.record.prevented).toBe(true)
  })

  test('Space stays inert with no open message or nothing to scroll', () => {
    const scrolled: number[] = []
    // No selected message: the pane callback is never consulted.
    dispatchMailKey(
      stubEvent({ key: ' ' }).event,
      baseCtx({
        hasSelectedMessage: false,
        onScrollMessagePane: (direction) => {
          scrolled.push(direction)
          return true
        },
      }),
    )
    expect(scrolled).toEqual([])
    // Nothing overflows: the key falls through unprevented.
    const { event, record } = stubEvent({ key: ' ' })
    dispatchMailKey(event, baseCtx({ onScrollMessagePane: () => false }))
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

describe('escape tier', () => {
  test('Escape with an open reader closes it and keeps the cursor', () => {
    let closed = 0
    let clearedSearch = 0
    const { event, record } = stubEvent({ key: 'Escape' })
    dispatchMailKey(
      event,
      baseCtx({
        hasOpenedMessage: true,
        hasSearchQuery: true,
        onCloseReader: () => {
          closed += 1
        },
        onClearSearchQuery: () => {
          clearedSearch += 1
        },
      }),
    )
    expect(closed).toBe(1)
    expect(clearedSearch).toBe(0)
    expect(record.prevented).toBe(true)
  })

  test('Escape with no open reader falls through to clearing the search', () => {
    let closed = 0
    let clearedSearch = 0
    dispatchMailKey(
      stubEvent({ key: 'Escape' }).event,
      baseCtx({
        hasOpenedMessage: false,
        hasSearchQuery: true,
        onCloseReader: () => {
          closed += 1
        },
        onClearSearchQuery: () => {
          clearedSearch += 1
        },
      }),
    )
    expect(closed).toBe(0)
    expect(clearedSearch).toBe(1)
  })
})
