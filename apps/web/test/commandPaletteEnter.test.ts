import { describe, expect, it } from 'bun:test'

import { resolvePaletteEnter } from '../src/components/command-palette/model'

describe('command palette Enter resolution', () => {
  it('applies the app-wide filter only on Shift+Enter', () => {
    expect(
      resolvePaletteEnter({
        shiftKey: true,
        hasHighlightedItem: false,
        hasItems: true,
      }),
    ).toBe('apply')
    // Shift wins even when an item is highlighted.
    expect(
      resolvePaletteEnter({
        shiftKey: true,
        hasHighlightedItem: true,
        hasItems: true,
      }),
    ).toBe('apply')
  })

  it('runs the highlighted item on plain Enter', () => {
    expect(
      resolvePaletteEnter({
        shiftKey: false,
        hasHighlightedItem: true,
        hasItems: true,
      }),
    ).toBe('run')
  })

  it('navigates into the results on Enter when nothing is highlighted', () => {
    expect(
      resolvePaletteEnter({
        shiftKey: false,
        hasHighlightedItem: false,
        hasItems: true,
      }),
    ).toBe('navigate')
  })

  it('is a no-op on Enter when there are no results to navigate to', () => {
    expect(
      resolvePaletteEnter({
        shiftKey: false,
        hasHighlightedItem: false,
        hasItems: false,
      }),
    ).toBe('none')
  })
})
