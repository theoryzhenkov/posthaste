/**
 * App-level appearance/theme preferences — the wire shape of `[appearance]` in
 * `app.toml`. TOML is the source of truth; the renderer keeps a derived
 * `localStorage` mirror only for fast boot (no FOUC). These types model the
 * daemon wire schema exactly.
 *
 * @spec docs/eph/DESIGN-L2-appearance-toml
 */
export type ThemeMode = 'light' | 'dark' | 'system'
export type UiDensity = 'compact' | 'cozy' | 'comfortable'
export type PalettePresetId =
  | 'neutral'
  | 'paperInk'
  | 'brutalist'
  | 'glass'
  | 'acid'
  | 'marzipan'
  | 'botanical'

/** One decorative bloom in a {@link GlassTheme}. */
export interface GlassBloom {
  id: string
  hue: number
  x: number
  y: number
  opacity: number
  radius: number
}

/** Advanced glass-theme parameters: decorative "blooms" rendered as the background. */
export interface GlassTheme {
  blooms: GlassBloom[]
}

/** UI appearance prefs. All fields optional; an absent/cleared field means "use the renderer default". */
export interface Appearance {
  mode?: ThemeMode | null
  palettePreset?: PalettePresetId | null
  density?: UiDensity | null
  accentHue?: number | null
  glassTheme?: GlassTheme | null
}
