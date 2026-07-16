import { describe, expect, it } from 'bun:test'
import { Briefcase, Tag } from 'lucide-react'

import {
  resolveTagStyle,
  TAG_COLOR_SWATCHES,
  TAG_ICON_NAMES,
  TAG_ICONS,
} from '../src/components/tags/model'

describe('resolveTagStyle', () => {
  it('applies explicit fg/bg/icon overrides', () => {
    const style = resolveTagStyle('work', {
      name: 'work',
      fg: '#1f2937',
      bg: '#dbeafe',
      icon: 'briefcase',
    })
    expect(style.fg).toBe('#1f2937')
    expect(style.bg).toBe('#dbeafe')
    expect(style.Icon).toBe(Briefcase)
  })

  it('falls back to a name-derived tint and the generic icon', () => {
    const style = resolveTagStyle('newsletters')
    expect(style.Icon).toBe(Tag)
    // bg is a transparent tint of the resolved accent, not a hard color
    expect(style.bg).toContain('color-mix')
    expect(style.fg).toBe(style.fg) // a concrete color string
    expect(typeof style.fg).toBe('string')
  })

  it('ignores an unknown icon name, keeping the default', () => {
    const style = resolveTagStyle('x', { name: 'x', icon: 'not-a-real-icon' })
    expect(style.Icon).toBe(Tag)
  })

  it('every curated swatch and icon key resolves to its component', () => {
    expect(TAG_COLOR_SWATCHES.length).toBeGreaterThan(0)
    for (const name of TAG_ICON_NAMES) {
      const style = resolveTagStyle('t', { name: 't', icon: name })
      expect(style.Icon).toBe(TAG_ICONS[name])
    }
  })
})
