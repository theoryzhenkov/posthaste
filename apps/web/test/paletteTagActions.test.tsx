import { describe, expect, it } from 'bun:test'
import { renderHook } from '@testing-library/react'

import {
  usePaletteActions,
  type PaletteActionHandlers,
} from '../src/components/command-palette/usePaletteActions'
import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

function makeHandlers(): {
  handlers: PaletteActionHandlers
  added: string[]
  removed: string[]
} {
  const added: string[] = []
  const removed: string[] = []
  const noop = () => {}
  const handlers: PaletteActionHandlers = {
    onAddTag: (tag) => added.push(tag),
    onRemoveTag: (tag) => removed.push(tag),
    onApplySearch: noop,
    onArchive: noop,
    onCompose: noop,
    onOpenSettings: noop,
    onOpenShortcuts: noop,
    onPlaceholderAction: noop,
    onReply: noop,
    onSelectMessage: noop,
    onSelectSmartMailbox: noop,
    onSelectSourceMailbox: noop,
    onToggleFlag: noop,
    replaceQuery: noop,
  }
  return { handlers, added, removed }
}

describe('usePaletteActions tag routing', () => {
  it('routes add-tag-to-message to onAddTag', () => {
    const { handlers, added } = makeHandlers()
    const { result } = renderHook(() => usePaletteActions(handlers))
    result.current({ kind: 'add-tag-to-message', tag: 'urgent' })
    expect(added).toEqual(['urgent'])
  })

  it('routes remove-tag-from-message to onRemoveTag', () => {
    const { handlers, removed } = makeHandlers()
    const { result } = renderHook(() => usePaletteActions(handlers))
    result.current({ kind: 'remove-tag-from-message', tag: 'work' })
    expect(removed).toEqual(['work'])
  })
})
