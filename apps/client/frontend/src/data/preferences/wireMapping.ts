import type { Appearance, ThemeColors as WireThemeColors } from '@/gen'
import {
  normalizeAccentHue,
  normalizeGlassThemeParameters,
  type ThemeMode,
  type UiDensity,
} from '@/lib/design'
import {
  defaultThemeColors,
  defaultThemePreferences,
  type DesignThemePreferences,
  type ThemeColors,
} from '@/lib/design/theme/themeSettings'

import { normalizeAppearancePreferences } from './storage'

const DEFAULT_APPEARANCE = defaultThemePreferences()

function wireColorsToDesign(
  colors: WireThemeColors | null | undefined,
  fallback: ThemeColors,
): ThemeColors {
  return {
    accentHue:
      colors?.accentHue != null
        ? normalizeAccentHue(colors.accentHue)
        : fallback.accentHue,
    surfaceHue:
      colors?.surfaceHue != null
        ? normalizeAccentHue(colors.surfaceHue)
        : fallback.surfaceHue,
  }
}

/**
 * Map a wire `Appearance` (nullable fields; the TOML source of truth) to the
 * renderer's `DesignThemePreferences` (required fields). Absent/cleared fields
 * fall back to the renderer defaults. Lossless for the curated knobs (per-mode
 * accent + surface, free-form theme id, density, glass blooms); the open
 * `tokens` escape hatch is carried by the wire but not yet consumed here.
 */
export function wireAppearanceToDesign(
  appearance: Appearance | null | undefined,
): DesignThemePreferences {
  return {
    mode: (appearance?.mode ?? DEFAULT_APPEARANCE.mode) as ThemeMode,
    theme: appearance?.theme ?? DEFAULT_APPEARANCE.theme,
    density: (appearance?.density ?? DEFAULT_APPEARANCE.density) as UiDensity,
    light: wireColorsToDesign(appearance?.light, defaultThemeColors()),
    dark: wireColorsToDesign(appearance?.dark, defaultThemeColors()),
    glassTheme: wireGlassThemeToDesign(
      appearance?.glassTheme,
      DEFAULT_APPEARANCE.glassTheme,
    ),
  }
}

/** The bloom fields that cross the wire unchanged, copied field-by-field so
 *  neither side leaks extra properties into the other's shape. */
function copyBloomFields<
  T extends {
    id: string
    hue: number
    x: number
    y: number
    opacity: number
    radius: number
  },
>(bloom: T) {
  return {
    id: bloom.id,
    hue: bloom.hue,
    x: bloom.x,
    y: bloom.y,
    opacity: bloom.opacity,
    radius: bloom.radius,
  }
}

function wireGlassThemeToDesign(
  glass: Appearance['glassTheme'] | undefined,
  fallback: DesignThemePreferences['glassTheme'],
): DesignThemePreferences['glassTheme'] {
  if (!glass || !glass.blooms?.length) {
    return fallback
  }
  return normalizeGlassThemeParameters({
    blooms: glass.blooms.map(copyBloomFields),
  })
}

function designColorsToWire(colors: ThemeColors): WireThemeColors {
  // The open `tokens` escape hatch is not authored in the renderer; the
  // curated knobs are the write surface.
  return { accentHue: colors.accentHue, surfaceHue: colors.surfaceHue, tokens: {} }
}

/**
 * Map the renderer's `DesignThemePreferences` to a wire `Appearance` (all
 * curated fields set, non-null) for PATCHing to TOML.
 */
export function designToWireAppearance(
  prefs: DesignThemePreferences,
): Appearance {
  return {
    mode: prefs.mode,
    theme: prefs.theme,
    density: prefs.density,
    light: designColorsToWire(prefs.light),
    dark: designColorsToWire(prefs.dark),
    glassTheme: {
      blooms: prefs.glassTheme.blooms.map(copyBloomFields),
    },
  }
}

/** Stable signature for comparing two appearance prefs (ignoring field order). */
export function appearanceSignature(prefs: DesignThemePreferences): string {
  return JSON.stringify(normalizeAppearancePreferences(prefs))
}

/**
 * Whether the design prefs equal the renderer defaults (no user customization) —
 * used to skip a no-op one-time import when TOML is unset.
 */
export function isDefaultAppearance(prefs: DesignThemePreferences): boolean {
  return appearanceSignature(prefs) === appearanceSignature(DEFAULT_APPEARANCE)
}
