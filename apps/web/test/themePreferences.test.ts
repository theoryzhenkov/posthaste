import { describe, expect, it } from 'bun:test'

import type { AppAppearanceSettings } from '../src/api/types'
import {
  appAppearanceFromPreferences,
  preferencesFromAppAppearance,
  shouldMigrateStoredThemePreferences,
  type DesignThemePreferences,
} from '../src/themeSettings'
import { defaultGlassThemeParameters } from '../src/design'

function defaultAppearance(): AppAppearanceSettings {
  return {
    mode: 'dark',
    palettePreset: 'neutral',
    density: 'compact',
    accentHue: 45,
    glassTheme: {
      blooms: defaultGlassThemeParameters.blooms.map((bloom) => ({ ...bloom })),
    },
  }
}

function storedPreferences(
  overrides: Partial<DesignThemePreferences> = {},
): DesignThemePreferences {
  return {
    mode: 'dark',
    palettePreset: 'glass',
    density: 'cozy',
    accentHue: 210,
    glassTheme: defaultGlassThemeParameters,
    ...overrides,
  }
}

describe('theme preference settings migration', () => {
  it('migrates non-default local preferences only when backend appearance is still default', () => {
    expect(
      shouldMigrateStoredThemePreferences(
        defaultAppearance(),
        storedPreferences(),
      ),
    ).toBe(true)

    expect(
      shouldMigrateStoredThemePreferences(
        { ...defaultAppearance(), palettePreset: 'glass' },
        storedPreferences(),
      ),
    ).toBe(false)
  })

  it('round trips app appearance settings through normalized design preferences', () => {
    const settings = appAppearanceFromPreferences(storedPreferences())
    const preferences = preferencesFromAppAppearance(settings)

    expect(preferences.palettePreset).toBe('glass')
    expect(preferences.density).toBe('cozy')
    expect(preferences.accentHue).toBe(210)
    expect(preferences.glassTheme.blooms.length).toBe(
      defaultGlassThemeParameters.blooms.length,
    )
  })
})
