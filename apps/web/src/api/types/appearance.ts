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

/**
 * Per-mode color overrides. The named knobs are the curated UX; `tokens` is the
 * open escape hatch (arbitrary CSS custom-property overrides) — the foundation
 * for user-supplied themes / imported CSS.
 */
export interface ThemeColors {
  accentHue?: number | null
  surfaceHue?: number | null
  tokens?: Record<string, string> | null
}

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

/**
 * UI appearance prefs. All fields optional; an absent/cleared field means "use
 * the renderer default".
 *
 * `theme` is a free-form id (built-ins `'neutral'` ("Classic") / `'glass'`; user
 * themes are any id) so user-created themes need no schema change. Light/dark
 * colors are customized independently.
 */
export interface Appearance {
  mode?: ThemeMode | null
  theme?: string | null
  density?: UiDensity | null
  light?: ThemeColors | null
  dark?: ThemeColors | null
  glassTheme?: GlassTheme | null
}
