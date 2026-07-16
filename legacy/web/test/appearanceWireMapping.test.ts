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
      theme: 'glass',
      density: 'cozy',
      light: { accentHue: 120 },
      glassTheme: null,
    }
    const design = wireAppearanceToDesign(wire)
    expect(design.mode).toBe('light')
    expect(design.theme).toBe('glass') // free-form theme id, preserved
    expect(design.density).toBe('cozy')
    expect(design.light.accentHue).toBe(120) // per-mode accent, light
    expect(design.light.surfaceHue).toBe(defaults.light.surfaceHue) // unset -> default
    expect(design.dark).toEqual(defaults.dark) // dark unset -> defaults
    expect(design.glassTheme).toEqual(defaults.glassTheme) // null -> default blooms
  })

  it('maps light and dark colors independently', () => {
    const design = wireAppearanceToDesign({
      light: { accentHue: 120, surfaceHue: 30 },
      dark: { accentHue: 300, surfaceHue: 250 },
    })
    expect(design.light).toEqual({ accentHue: 120, surfaceHue: 30 })
    expect(design.dark).toEqual({ accentHue: 300, surfaceHue: 250 })
  })

  it('preserves an unknown (user) theme id', () => {
    expect(wireAppearanceToDesign({ theme: 'my-custom-theme' }).theme).toBe(
      'my-custom-theme',
    )
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
    const original = {
      ...defaults,
      mode: 'light' as const,
      theme: 'glass',
      light: { accentHue: 200, surfaceHue: 30 },
      dark: { accentHue: 280, surfaceHue: 250 },
    }
    const wire = designToWireAppearance(original)
    expect(wire.mode).toBe('light')
    expect(wire.theme).toBe('glass')
    expect(wire.light?.accentHue).toBe(200)
    expect(wire.light?.surfaceHue).toBe(30)
    expect(wire.dark?.accentHue).toBe(280)
    expect(wire.dark?.surfaceHue).toBe(250)
    expect(wire.glassTheme?.blooms.length).toBe(
      defaults.glassTheme.blooms.length,
    )

    const back = wireAppearanceToDesign(wire)
    expect(appearanceSignature(back)).toBe(appearanceSignature(original))
  })

  it('detects default vs customized appearance', () => {
    expect(isDefaultAppearance(defaults)).toBe(true)
    expect(
      isDefaultAppearance({
        ...defaults,
        light: { ...defaults.light, accentHue: 200 },
      }),
    ).toBe(false)
    expect(isDefaultAppearance({ ...defaults, mode: 'light' })).toBe(false)
    expect(isDefaultAppearance({ ...defaults, theme: 'glass' })).toBe(false)
  })
})
