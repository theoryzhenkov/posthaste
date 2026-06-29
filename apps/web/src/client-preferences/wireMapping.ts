import type { Appearance } from '@/api/types'
import {
  normalizeAccentHue,
  normalizeGlassThemeParameters,
  type PalettePresetId,
  type ThemeMode,
  type UiDensity,
} from '@/design'
import {
  defaultThemePreferences,
  type DesignThemePreferences,
} from '@/themeSettings'

import { normalizeAppearancePreferences } from './storage'

const DEFAULT_APPEARANCE = defaultThemePreferences()

/**
 * Map a wire `Appearance` (nullable fields; the TOML source of truth) to the
 * renderer's `DesignThemePreferences` (required fields). Absent/cleared fields
 * fall back to the renderer defaults. Wire enum values are the same strings as
 * the `@/design` enums, so a structural cast bridges the nominal types.
 *
 * @spec docs/eph/DESIGN-L2-appearance-toml
 */
export function wireAppearanceToDesign(
  appearance: Appearance | null | undefined,
): DesignThemePreferences {
  return {
    accentHue:
      appearance?.accentHue != null
        ? normalizeAccentHue(appearance.accentHue)
        : DEFAULT_APPEARANCE.accentHue,
    mode: (appearance?.mode ?? DEFAULT_APPEARANCE.mode) as ThemeMode,
    palettePreset: (appearance?.palettePreset ??
      DEFAULT_APPEARANCE.palettePreset) as PalettePresetId,
    density: (appearance?.density ?? DEFAULT_APPEARANCE.density) as UiDensity,
    glassTheme: wireGlassThemeToDesign(
      appearance?.glassTheme,
      DEFAULT_APPEARANCE.glassTheme,
    ),
  }
}

function wireGlassThemeToDesign(
  glass: Appearance['glassTheme'],
  fallback: DesignThemePreferences['glassTheme'],
): DesignThemePreferences['glassTheme'] {
  if (!glass || !glass.blooms?.length) {
    return fallback
  }
  return normalizeGlassThemeParameters({
    blooms: glass.blooms.map((bloom) => ({
      id: bloom.id,
      hue: bloom.hue,
      x: bloom.x,
      y: bloom.y,
      opacity: bloom.opacity,
      radius: bloom.radius,
    })),
  })
}

/**
 * Map the renderer's `DesignThemePreferences` to a wire `Appearance` (all fields
 * set, non-null) for PATCHing to TOML.
 *
 * @spec docs/eph/DESIGN-L2-appearance-toml
 */
export function designToWireAppearance(
  prefs: DesignThemePreferences,
): Appearance {
  return {
    accentHue: prefs.accentHue,
    mode: prefs.mode,
    palettePreset: prefs.palettePreset,
    density: prefs.density,
    glassTheme: {
      blooms: prefs.glassTheme.blooms.map((bloom) => ({
        id: bloom.id,
        hue: bloom.hue,
        x: bloom.x,
        y: bloom.y,
        opacity: bloom.opacity,
        radius: bloom.radius,
      })),
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
  const normalized = normalizeAppearancePreferences(prefs)
  return (
    normalized.accentHue === DEFAULT_APPEARANCE.accentHue &&
    normalized.mode === DEFAULT_APPEARANCE.mode &&
    normalized.palettePreset === DEFAULT_APPEARANCE.palettePreset &&
    normalized.density === DEFAULT_APPEARANCE.density &&
    JSON.stringify(normalized.glassTheme) ===
      JSON.stringify(DEFAULT_APPEARANCE.glassTheme)
  )
}
