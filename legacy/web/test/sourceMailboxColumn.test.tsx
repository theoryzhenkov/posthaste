import { describe, expect, it } from 'bun:test'
import { act, renderHook } from '@testing-library/react'

import {
  ALL_COLUMNS,
  DEFAULT_COLUMNS,
  buildGridTemplate,
  getColumnDef,
  SORTABLE_COLUMNS,
} from '../src/components/thread-list/columns'
import { useColumnConfig } from '../src/components/thread-list/useColumnConfig'
import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

describe('sourceMailbox column model', () => {
  it('is offered in the picker but hidden by default (user-managed visibility)', () => {
    expect(ALL_COLUMNS).toContain('sourceMailbox')
    expect(DEFAULT_COLUMNS).not.toContain('sourceMailbox')
  })

  it('sits adjacent to the source (account) column in picker order', () => {
    const sourceIndex = ALL_COLUMNS.indexOf('source')
    expect(ALL_COLUMNS[sourceIndex + 1]).toBe('sourceMailbox')
  })

  it('mirrors the source column shape: fixed, resizable, with a min width', () => {
    const def = getColumnDef('sourceMailbox')
    expect(def.label).toBe('Mailbox')
    expect(def.kind).toBe('fixed')
    expect(def.resizable).toBe(true)
    expect(def.minWidth).toBeGreaterThan(0)
    expect(def.basis).toBeGreaterThanOrEqual(def.minWidth ?? 0)
  })

  it('is not server-sortable (mailbox has no sort field)', () => {
    expect(SORTABLE_COLUMNS.has('sourceMailbox')).toBe(false)
  })

  it('contributes its own fixed track to the grid template when active', () => {
    const withColumn = buildGridTemplate([...DEFAULT_COLUMNS, 'sourceMailbox'])
    const without = buildGridTemplate(DEFAULT_COLUMNS)
    const def = getColumnDef('sourceMailbox')
    expect(withColumn).toBe(`${without} ${def.basis}px`)
  })

  it('honours a persisted resize in the grid template, like its siblings', () => {
    const def = getColumnDef('sourceMailbox')
    const template = buildGridTemplate([...DEFAULT_COLUMNS, 'sourceMailbox'], {
      sourceMailbox: def.basis + 40,
    })
    expect(template.endsWith(`${def.basis + 40}px`)).toBe(true)
  })
})

describe('sourceMailbox column visibility (standard column mechanism)', () => {
  it('toggles on and off through useColumnConfig.toggleColumn', () => {
    const { result } = renderHook(() => useColumnConfig())
    expect(result.current.columns).not.toContain('sourceMailbox')

    act(() => {
      result.current.toggleColumn('sourceMailbox')
    })
    expect(result.current.columns).toContain('sourceMailbox')

    act(() => {
      result.current.toggleColumn('sourceMailbox')
    })
    expect(result.current.columns).not.toContain('sourceMailbox')
  })
})
