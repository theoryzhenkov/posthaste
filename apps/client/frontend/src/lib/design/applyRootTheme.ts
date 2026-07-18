import {
  defaultAccentHue,
  defaultSurfaceHue,
  normalizeAccentHue,
} from './tokens/accent'
import { designClassNames, designDataAttributes } from './tokens/attributes'
import { defaultUiDensity, type UiDensity } from './tokens/density'
import {
  defaultGlassThemeParameters,
  glassMeshBackground,
  type GlassThemeParameters,
} from './theme/glassTheme'
import {
  defaultThemeId,
  defaultThemeMode,
  themeStyle,
  type ResolvedThemeMode,
  type ThemeMode,
} from './theme/theme'

/** Per-mode color knobs (mirrors `ThemeColors` in `themeSettings`). */
export type RootThemeColors = {
  readonly accentHue?: number
  readonly surfaceHue?: number
}

export type RootThemeState = {
  readonly mode?: ThemeMode
  readonly theme?: string
  readonly density?: UiDensity
  readonly light?: RootThemeColors
  readonly dark?: RootThemeColors
  readonly glassTheme?: GlassThemeParameters
}

export type AppliedThemeColors = {
  readonly accentHue: number
  readonly surfaceHue: number
}

export type AppliedRootTheme = {
  readonly mode: ThemeMode
  readonly resolvedMode: ResolvedThemeMode
  readonly theme: string
  readonly density: UiDensity
  readonly light: AppliedThemeColors
  readonly dark: AppliedThemeColors
  readonly glassTheme: GlassThemeParameters
}

export function resolveThemeMode(
  mode: ThemeMode,
  systemMode: ResolvedThemeMode,
): ResolvedThemeMode {
  return mode === 'system' ? systemMode : mode
}

export function getSystemThemeMode(
  matchMedia: Window['matchMedia'] | undefined = globalThis.matchMedia,
): ResolvedThemeMode {
  if (matchMedia?.('(prefers-color-scheme: dark)').matches) {
    return 'dark'
  }
  return 'light'
}

function resolveColors(
  colors: RootThemeColors | undefined,
): AppliedThemeColors {
  return {
    accentHue: normalizeAccentHue(colors?.accentHue ?? defaultAccentHue),
    surfaceHue: normalizeAccentHue(colors?.surfaceHue ?? defaultSurfaceHue),
  }
}

export function applyRootTheme(
  root: HTMLElement,
  state: RootThemeState,
  systemMode: ResolvedThemeMode = getSystemThemeMode(),
): AppliedRootTheme {
  const mode = state.mode ?? defaultThemeMode
  const theme = state.theme ?? defaultThemeId
  const density = state.density ?? defaultUiDensity
  const light = resolveColors(state.light)
  const dark = resolveColors(state.dark)
  const glassTheme = state.glassTheme ?? defaultGlassThemeParameters
  const resolvedMode = resolveThemeMode(mode, systemMode)
  const colors = resolvedMode === 'dark' ? dark : light
  const accentHue = colors.accentHue
  const accent = `oklch(0.68 0.17 ${accentHue})`
  const accentStrong = `oklch(0.72 0.16 ${accentHue})`
  const accentDeep = `oklch(0.50 0.18 ${accentHue})`
  const accentSoft = `oklch(0.90 0.07 ${accentHue} / 0.74)`
  const accentGlassSoft =
    resolvedMode === 'dark'
      ? `oklch(0.40 0.10 ${accentHue} / 0.58)`
      : `oklch(0.90 0.075 ${accentHue} / 0.56)`
  const accentForeground = `oklch(0.14 0.035 ${accentHue})`

  root.setAttribute(designDataAttributes.themeMode, mode)
  root.setAttribute(designDataAttributes.resolvedThemeMode, resolvedMode)
  root.setAttribute(designDataAttributes.palettePreset, theme)
  root.setAttribute(designDataAttributes.paletteStyle, themeStyle(theme))
  root.setAttribute(designDataAttributes.uiDensity, density)
  root.classList.toggle(designClassNames.dark, resolvedMode === 'dark')
  root.style.setProperty('--ph-accent-hue', String(accentHue))
  // Surface hue + chroma drive the parameterized surface tokens. At the default
  // hue the chroma stays near-grey (preserving the shipped neutral look); any
  // custom hue lifts the chroma so the chosen color visibly tints the surfaces.
  const surfaceCustomized = colors.surfaceHue !== defaultSurfaceHue
  const surfaceChroma = surfaceCustomized
    ? resolvedMode === 'dark'
      ? 0.03
      : 0.022
    : resolvedMode === 'dark'
      ? 0.008
      : 0.007
  root.style.setProperty('--ph-surface-hue', String(colors.surfaceHue))
  root.style.setProperty('--ph-surface-chroma', String(surfaceChroma))
  root.style.setProperty(
    '--primary',
    resolvedMode === 'dark' ? accentStrong : accent,
  )
  root.style.setProperty('--primary-foreground', accentForeground)
  root.style.setProperty('--ring', accentStrong)
  root.style.setProperty('--sidebar-primary', accentStrong)
  root.style.setProperty('--sidebar-ring', accentStrong)
  root.style.setProperty('--signal-flag', accentStrong)
  root.style.setProperty('--brand-coral', accentStrong)
  root.style.setProperty('--brand-coral-foreground', accentForeground)
  root.style.setProperty(
    '--brand-coral-soft',
    theme === 'glass' ? accentGlassSoft : accentSoft,
  )
  root.style.setProperty('--brand-coral-deep', accentDeep)
  root.style.setProperty(
    '--list-selection',
    resolvedMode === 'dark'
      ? `oklch(0.48 0.13 ${accentHue} / 0.44)`
      : `oklch(0.80 0.10 ${accentHue} / 0.46)`,
  )
  root.style.setProperty(
    '--list-selection-foreground',
    resolvedMode === 'dark'
      ? 'oklch(0.98 0.01 292)'
      : `oklch(0.20 0.07 ${accentHue})`,
  )
  root.style.setProperty(
    '--focus-soft',
    resolvedMode === 'dark'
      ? `oklch(0.72 0.16 ${accentHue} / 0.34)`
      : `oklch(0.72 0.16 ${accentHue} / 0.24)`,
  )
  root.style.setProperty(
    '--glass-mesh-background',
    glassMeshBackground(glassTheme, resolvedMode),
  )

  return {
    mode,
    resolvedMode,
    theme,
    density,
    light,
    dark,
    glassTheme,
  }
}
