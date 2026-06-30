import { describe, expect, it } from 'bun:test'

import { setupDomEnvironment } from './dom-env'
import {
  dispatchMailKey,
  PANE_ORDER,
  type KeyboardDispatchContext,
  type PaneId,
  type PaneKeyHandler,
} from '../src/components/keyboard/dispatch'

setupDomEnvironment()

type Calls = Record<string, number>

function makeCtx(over: Partial<KeyboardDispatchContext> = {}): {
  ctx: KeyboardDispatchContext
  calls: Calls
  focused: PaneId[]
  gotos: { role: string; forceSmart: boolean }[]
} {
  const calls: Calls = {}
  const focused: PaneId[] = []
  const prefix = { value: null as KeyboardDispatchContext['pendingPrefix'] }
  const gotos: { role: string; forceSmart: boolean }[] = []
  const bump = (name: string) => () => {
    calls[name] = (calls[name] ?? 0) + 1
  }
  const ctx: KeyboardDispatchContext = {
    effectiveSurfaceOpen: false,
    overlayOwnsInput: false,
    hasSelectedMessage: false,
    hasSearchQuery: false,
    activePane: 'list',
    availablePanes: PANE_ORDER,
    focusPane: (pane) => focused.push(pane),
    resolvePaneHandler: () => undefined,
    get pendingPrefix() {
      return prefix.value
    },
    setPendingPrefix: (next) => {
      prefix.value = next
    },
    onGoto: (role, options) =>
      gotos.push({ role, forceSmart: options.forceSmart }),
    onGotoConversation: bump('gotoConversation'),
    onOpenCommandPalette: bump('palette'),
    onOpenSettings: bump('settings'),
    onCompose: bump('compose'),
    onReply: bump('reply'),
    onReplyAll: bump('replyAll'),
    onToggleFlag: bump('toggleFlag'),
    onUndo: bump('undo'),
    onRedo: bump('redo'),
    onArchive: bump('archive'),
    onTrash: bump('trash'),
    onOpenTagEditor: bump('tag'),
    onOpenFocusedMessage: bump('open'),
    onClearSelectedMessage: bump('clearSelection'),
    onClearSearchQuery: bump('clearSearch'),
    onToggleShortcuts: bump('shortcuts'),
    ...over,
  }
  return { ctx, calls, focused, gotos }
}

function key(init: Partial<KeyboardEvent> & { key: string }): KeyboardEvent {
  let prevented = false
  return {
    metaKey: false,
    ctrlKey: false,
    shiftKey: false,
    altKey: false,
    target: null,
    ...init,
    preventDefault: () => {
      prevented = true
    },
    get defaultPrevented() {
      return prevented
    },
  } as unknown as KeyboardEvent
}

describe('dispatchMailKey', () => {
  it('opens the command palette on the command-k chord', () => {
    const { ctx, calls } = makeCtx()
    dispatchMailKey(key({ key: 'k', metaKey: true }), ctx)
    expect(calls.palette).toBe(1)
  })

  it('lets command chords fire even inside a text input', () => {
    const input = document.createElement('input')
    const { ctx, calls } = makeCtx()
    dispatchMailKey(
      key({ key: 'n', metaKey: true, target: input as never }),
      ctx,
    )
    expect(calls.compose).toBe(1)
  })

  it('rotates pane focus right with Shift+L, wrapping past the last pane', () => {
    const { ctx, focused } = makeCtx({ activePane: 'detail' })
    dispatchMailKey(key({ key: 'L', shiftKey: true }), ctx)
    expect(focused).toEqual(['sidebar'])
  })

  it('rotates pane focus left with Shift+H, wrapping before the first pane', () => {
    const { ctx, focused } = makeCtx({ activePane: 'sidebar' })
    dispatchMailKey(key({ key: 'H', shiftKey: true }), ctx)
    expect(focused).toEqual(['detail'])
  })

  it('routes j/k to the focused pane handler', () => {
    let seen: string | null = null
    const handler: PaneKeyHandler = (event) => {
      seen = event.key
      return true
    }
    const { ctx, focused } = makeCtx({ resolvePaneHandler: () => handler })
    dispatchMailKey(key({ key: 'j' }), ctx)
    expect(seen).toBe('j')
    // j is not a focus-movement key, so no pane change.
    expect(focused).toEqual([])
  })

  it('routes plain h/l to the focused pane handler (tree collapse/expand)', () => {
    const seen: string[] = []
    const handler: PaneKeyHandler = (event) => {
      seen.push(event.key)
      return true
    }
    const { ctx, focused } = makeCtx({ resolvePaneHandler: () => handler })
    dispatchMailKey(key({ key: 'h' }), ctx)
    dispatchMailKey(key({ key: 'l' }), ctx)
    expect(seen).toEqual(['h', 'l'])
    // Plain h/l are not pane-rotation keys, so focus does not move.
    expect(focused).toEqual([])
  })

  it('opens the tag editor on t only with a selection', () => {
    const without = makeCtx({ hasSelectedMessage: false })
    dispatchMailKey(key({ key: 't' }), without.ctx)
    expect(without.calls.tag).toBeUndefined()

    const withSel = makeCtx({ hasSelectedMessage: true })
    dispatchMailKey(key({ key: 't' }), withSel.ctx)
    expect(withSel.calls.tag).toBe(1)
  })

  it('archives on e and trashes on # when a message is selected', () => {
    const { ctx, calls } = makeCtx({ hasSelectedMessage: true })
    dispatchMailKey(key({ key: 'e' }), ctx)
    dispatchMailKey(key({ key: '#' }), ctx)
    expect(calls.archive).toBe(1)
    expect(calls.trash).toBe(1)
  })

  it('is fully inert while a focused surface owns the screen', () => {
    const { ctx, calls, focused } = makeCtx({
      effectiveSurfaceOpen: true,
      hasSelectedMessage: true,
    })
    dispatchMailKey(key({ key: 'L', shiftKey: true }), ctx)
    dispatchMailKey(key({ key: 'e' }), ctx)
    dispatchMailKey(key({ key: 'k', metaKey: true }), ctx)
    expect(calls).toEqual({})
    expect(focused).toEqual([])
  })

  it('suppresses pane keys while a lightweight overlay owns input', () => {
    const { ctx, focused, calls } = makeCtx({
      overlayOwnsInput: true,
      hasSelectedMessage: true,
    })
    dispatchMailKey(key({ key: 'L', shiftKey: true }), ctx)
    dispatchMailKey(key({ key: 'e' }), ctx)
    expect(focused).toEqual([])
    expect(calls.archive).toBeUndefined()
    // but ? still toggles the reference so it can be closed
    dispatchMailKey(key({ key: '?' }), ctx)
    expect(calls.shortcuts).toBe(1)
  })

  it('runs a two-key goto (g then i) as a context-aware inbox jump', () => {
    const { ctx, gotos } = makeCtx()
    dispatchMailKey(key({ key: 'g' }), ctx)
    expect(ctx.pendingPrefix).toBe('g')
    dispatchMailKey(key({ key: 'i' }), ctx)
    expect(gotos).toEqual([{ role: 'inbox', forceSmart: false }])
    expect(ctx.pendingPrefix).toBeNull()
  })

  it('runs a three-key force-smart goto (g q t)', () => {
    const { ctx, gotos } = makeCtx()
    dispatchMailKey(key({ key: 'g' }), ctx)
    dispatchMailKey(key({ key: 'q' }), ctx)
    expect(ctx.pendingPrefix).toBe('gq')
    dispatchMailKey(key({ key: 't' }), ctx)
    expect(gotos).toEqual([{ role: 'trash', forceSmart: true }])
  })

  it('runs gc as a goto-conversation when a message is selected', () => {
    const { ctx, calls } = makeCtx({ hasSelectedMessage: true })
    dispatchMailKey(key({ key: 'g' }), ctx)
    dispatchMailKey(key({ key: 'c' }), ctx)
    expect(calls.gotoConversation).toBe(1)
    expect(ctx.pendingPrefix).toBeNull()
  })

  it('consumes gc but does nothing with no message selected', () => {
    const { ctx, calls } = makeCtx({ hasSelectedMessage: false })
    dispatchMailKey(key({ key: 'g' }), ctx)
    dispatchMailKey(key({ key: 'c' }), ctx)
    expect(calls.gotoConversation).toBeUndefined()
    expect(ctx.pendingPrefix).toBeNull()
  })

  it('drops a stray prefix and handles the next key normally', () => {
    const handlerSeen: string[] = []
    const { ctx, gotos } = makeCtx({
      resolvePaneHandler: () => (event) => {
        handlerSeen.push(event.key)
        return true
      },
    })
    dispatchMailKey(key({ key: 'g' }), ctx)
    // 'j' is not a goto target: prefix is dropped and j navigates the list.
    dispatchMailKey(key({ key: 'j' }), ctx)
    expect(gotos).toEqual([])
    expect(handlerSeen).toEqual(['j'])
    expect(ctx.pendingPrefix).toBeNull()
  })

  it('clears the selection before the search query on Escape', () => {
    const withSel = makeCtx({ hasSelectedMessage: true, hasSearchQuery: true })
    dispatchMailKey(key({ key: 'Escape' }), withSel.ctx)
    expect(withSel.calls.clearSelection).toBe(1)
    expect(withSel.calls.clearSearch).toBeUndefined()

    const searchOnly = makeCtx({ hasSearchQuery: true })
    dispatchMailKey(key({ key: 'Escape' }), searchOnly.ctx)
    expect(searchOnly.calls.clearSearch).toBe(1)
  })
})
