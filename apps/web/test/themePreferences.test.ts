import { describe, expect, it } from 'bun:test'

import {
  defaultAccentHue,
  defaultPalettePresetId,
  defaultThemeMode,
  defaultUiDensity,
  defaultGlassThemeParameters,
} from '../src/design'
import { defaultThemePreferences } from '../src/themeSettings'

// Appearance is now client-local presentation state persisted in localStorage
// (see docs/eph/DESIGN-L1-deployment-modes); it no longer round-trips through
// the daemon API, so the former server-appearance migration helpers are gone.
describe('default theme preferences', () => {
  it('matches the design defaults used to seed local storage', () => {
    const preferences = defaultThemePreferences()

    expect(preferences.mode).toBe(defaultThemeMode)
    expect(preferences.palettePreset).toBe(defaultPalettePresetId)
    expect(preferences.density).toBe(defaultUiDensity)
    expect(preferences.accentHue).toBe(defaultAccentHue)
    expect(preferences.glassTheme.blooms.length).toBe(
      defaultGlassThemeParameters.blooms.length,
    )
  })
})
