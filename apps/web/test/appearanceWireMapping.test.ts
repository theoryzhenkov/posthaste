import { describe, expect, it } from 'bun:test'

import type { Appearance } from '../src/api/types'
import {
  appearanceSignature,
  designToWireAppearance,
  isDefaultAppearance,
  wireAppearanceToDesign,
} from '../src/client-preferences/wireMapping'
import { defaultThemePreferences } from '../src/themeSettings'

describe('appearance wire mapping', () => {
  const defaults = defaultThemePreferences()

  it('maps a null/undefined/empty wire appearance to the renderer defaults', () => {
    expect(wireAppearanceToDesign(null)).toEqual(defaults)
    expect(wireAppearanceToDesign(undefined)).toEqual(defaults)
    expect(wireAppearanceToDesign({})).toEqual(defaults)
  })

  it('maps wire fields to design fields, filling defaults for cleared fields', () => {
    const wire: Appearance = {
      mode: 'light',
      palettePreset: 'paperInk',
      density: 'cozy',
      accentHue: 120,
      glassTheme: null,
    }
    const design = wireAppearanceToDesign(wire)
    expect(design.mode).toBe('light')
    expect(design.palettePreset).toBe('paperInk')
    expect(design.density).toBe('cozy')
    expect(design.accentHue).toBe(120)
    expect(design.glassTheme).toEqual(defaults.glassTheme) // null -> default blooms
  })

  it('preserves glass blooms through wire -> design', () => {
    const wire: Appearance = {
      glassTheme: {
        blooms: [{ id: 'b1', hue: 100, x: 5, y: 15, opacity: 0.4, radius: 30 }],
      },
    }
    const design = wireAppearanceToDesign(wire)
    expect(design.glassTheme.blooms.length).toBe(1)
    expect(design.glassTheme.blooms[0].id).toBe('b1')
    expect(design.glassTheme.blooms[0].hue).toBe(100)
  })

  it('round-trips design -> wire -> design for a full appearance', () => {
    const original = { ...defaults, mode: 'light' as const, accentHue: 200 }
    const wire = designToWireAppearance(original)
    expect(wire.mode).toBe('light')
    expect(wire.accentHue).toBe(200)
    expect(wire.glassTheme?.blooms.length).toBe(
      defaults.glassTheme.blooms.length,
    )

    const back = wireAppearanceToDesign(wire)
    expect(appearanceSignature(back)).toBe(appearanceSignature(original))
  })

  it('detects default vs customized appearance', () => {
    expect(isDefaultAppearance(defaults)).toBe(true)
    expect(isDefaultAppearance({ ...defaults, accentHue: 200 })).toBe(false)
    expect(isDefaultAppearance({ ...defaults, mode: 'light' })).toBe(false)
  })
})
