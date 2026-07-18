import {
  defaultAccentHue,
  defaultSurfaceHue,
  defaultThemeId,
  defaultThemeMode,
  defaultUiDensity,
  normalizeGlassThemeParameters,
  type GlassThemeParameters,
  type ThemeMode,
  type UiDensity,
} from '@/lib/design'

/**
 * Per-mode color knobs. The accent (interactive/brand) hue + the surface (the
 * "main color" of panes/background) hue are customized independently for light
 * and dark, so a theme can read very differently between modes.
 */
export interface ThemeColors {
  accentHue: number
  surfaceHue: number
}

export interface DesignThemePreferences {
  mode: ThemeMode
  /** Free-form theme id; built-ins `'neutral'` ("Classic") / `'glass'`. */
  theme: string
  density: UiDensity
  light: ThemeColors
  dark: ThemeColors
  glassTheme: GlassThemeParameters
}

export function defaultThemeColors(): ThemeColors {
  return { accentHue: defaultAccentHue, surfaceHue: defaultSurfaceHue }
}

export function defaultThemePreferences(): DesignThemePreferences {
  return {
    mode: defaultThemeMode,
    theme: defaultThemeId,
    density: defaultUiDensity,
    light: defaultThemeColors(),
    dark: defaultThemeColors(),
    glassTheme: normalizeGlassThemeParameters(null),
  }
}
