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
 * fall back to the renderer defaults.
 *
 * NOTE — transitional bridge: the wire schema is per-mode (`light`/`dark`
 * colors) + free-form `theme`, but the design layer is still single-accent +
 * `palettePreset`. This collapses the per-mode accent to one (light, falling
 * back to dark) and reads `theme` as the palette id. The design-layer revamp
 * (per-mode colors + `surfaceHue` + `tokens`) will make this lossless.
 *
 * @spec docs/eph/DESIGN-L2-appearance-toml
 */
export function wireAppearanceToDesign(
  appearance: Appearance | null | undefined,
): DesignThemePreferences {
  const accent = appearance?.light?.accentHue ?? appearance?.dark?.accentHue
  return {
    accentHue:
      accent != null
        ? normalizeAccentHue(accent)
        : DEFAULT_APPEARANCE.accentHue,
    mode: (appearance?.mode ?? DEFAULT_APPEARANCE.mode) as ThemeMode,
    palettePreset: (appearance?.theme ??
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
  // Transitional: the single design accent is written to both modes so the
  // round-trip is stable until the design layer becomes per-mode.
  return {
    mode: prefs.mode,
    theme: prefs.palettePreset,
    density: prefs.density,
    light: { accentHue: prefs.accentHue },
    dark: { accentHue: prefs.accentHue },
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
