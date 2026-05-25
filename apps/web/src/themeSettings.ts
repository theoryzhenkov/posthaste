import {
  defaultAccentHue,
  defaultPalettePresetId,
  defaultThemeMode,
  defaultUiDensity,
  normalizeAccentHue,
  normalizeGlassThemeParameters,
  isPalettePresetId,
  isThemeMode,
  isUiDensity,
  type GlassThemeParameters,
  type PalettePresetId,
  type ThemeMode,
  type UiDensity,
} from '@/design'
import type { AppAppearanceSettings } from './api/types'

export interface DesignThemePreferences {
  accentHue: number
  density: UiDensity
  glassTheme: GlassThemeParameters
  mode: ThemeMode
  palettePreset: PalettePresetId
}

export function defaultThemePreferences(): DesignThemePreferences {
  return {
    accentHue: defaultAccentHue,
    glassTheme: normalizeGlassThemeParameters(null),
    mode: defaultThemeMode,
    palettePreset: defaultPalettePresetId,
    density: defaultUiDensity,
  }
}

export function appAppearanceFromPreferences(
  preferences: DesignThemePreferences,
): AppAppearanceSettings {
  return {
    mode: preferences.mode,
    palettePreset: preferences.palettePreset,
    density: preferences.density,
    accentHue: normalizeAccentHue(preferences.accentHue),
    glassTheme: {
      blooms: normalizeGlassThemeParameters(preferences.glassTheme).blooms.map(
        (bloom) => ({ ...bloom }),
      ),
    },
  }
}

export function preferencesFromAppAppearance(
  appearance: AppAppearanceSettings,
): DesignThemePreferences {
  return {
    mode: isThemeMode(appearance.mode) ? appearance.mode : defaultThemeMode,
    palettePreset: isPalettePresetId(appearance.palettePreset)
      ? appearance.palettePreset
      : defaultPalettePresetId,
    density: isUiDensity(appearance.density)
      ? appearance.density
      : defaultUiDensity,
    accentHue: normalizeAccentHue(appearance.accentHue),
    glassTheme: normalizeGlassThemeParameters(appearance.glassTheme),
  }
}

export function preferencesSignature(
  preferences: DesignThemePreferences,
): string {
  return JSON.stringify(appAppearanceFromPreferences(preferences))
}

export function shouldMigrateStoredThemePreferences(
  serverAppearance: AppAppearanceSettings,
  storedPreferences: DesignThemePreferences,
): boolean {
  const defaultSignature = preferencesSignature(defaultThemePreferences())
  return (
    preferencesSignature(preferencesFromAppAppearance(serverAppearance)) ===
      defaultSignature &&
    preferencesSignature(storedPreferences) !== defaultSignature
  )
}
