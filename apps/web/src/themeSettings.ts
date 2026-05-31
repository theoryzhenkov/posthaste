import {
  defaultAccentHue,
  defaultPalettePresetId,
  defaultThemeMode,
  defaultUiDensity,
  normalizeGlassThemeParameters,
  type GlassThemeParameters,
  type PalettePresetId,
  type ThemeMode,
  type UiDensity,
} from '@/design'

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
