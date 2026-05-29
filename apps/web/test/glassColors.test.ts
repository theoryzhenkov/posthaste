import { describe, expect, it } from 'bun:test'

import type { GlassBloom } from '../src/design/glassTheme'
import {
  glassBloomColor,
  glassBloomDisplayColor,
  glassMeshBackground,
} from '../src/design/glassTheme'

const bloom: GlassBloom = {
  id: 'b1',
  hue: 285,
  x: 20,
  y: 10,
  opacity: 0.35,
  radius: 45,
}

describe('glass theme colors', () => {
  it('uses mode-dependent lightness/chroma and carries hue + opacity', () => {
    expect(glassBloomColor(bloom, 'dark')).toBe('oklch(0.58 0.18 285 / 0.35)')
    expect(glassBloomColor(bloom, 'light')).toBe('oklch(0.72 0.15 285 / 0.35)')
  })

  it('normalizes the hue in the display swatch color', () => {
    expect(glassBloomDisplayColor({ ...bloom, hue: 405 })).toBe(
      'oklch(0.68 0.17 45)',
    )
  })

  it('composes a radial gradient per bloom over the mode base layer', () => {
    expect(glassMeshBackground({ blooms: [bloom] }, 'dark')).toBe(
      'radial-gradient(circle at 20% 10%, oklch(0.58 0.18 285 / 0.35) 0%, transparent 45%), ' +
        'linear-gradient(180deg, #0a0812 0%, #050410 100%)',
    )
  })
})
