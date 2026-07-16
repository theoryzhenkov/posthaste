import { describe, expect, it } from 'bun:test'

import { itemButtonClass } from '../src/components/sidebar/model'

describe('itemButtonClass — active-pane selection model', () => {
  it('applies the accent selection only when selected AND pane-active', () => {
    const cls = itemButtonClass(true, 0, true)
    expect(cls).toContain('bg-[var(--list-selection)]')
    expect(cls).toContain('text-[var(--list-selection-foreground)]')
    expect(cls).not.toContain('--list-selection-muted')
  })

  it('greys out the selection when selected but the pane is not focused', () => {
    const cls = itemButtonClass(true, 0, false)
    expect(cls).toContain('bg-[var(--list-selection-muted)]')
    expect(cls).toContain('text-[var(--list-selection-muted-foreground)]')
    expect(cls).not.toContain('bg-[var(--list-selection)]')
  })

  it('renders a plain row when nothing is selected (regardless of focus)', () => {
    expect(itemButtonClass(false, 0, true)).not.toContain('--list-selection')
    expect(itemButtonClass(false, 0, false)).not.toContain('--list-selection')
  })

  it('indents nested mailboxes by depth', () => {
    expect(itemButtonClass(false, 1, false)).toContain('pl-[22px]')
    expect(itemButtonClass(false, 0, false)).toContain('pl-2')
  })
})
