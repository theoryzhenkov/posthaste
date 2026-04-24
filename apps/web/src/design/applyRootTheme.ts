import { defaultAccentHue, normalizeAccentHue } from './accent'
import { designClassNames, designDataAttributes } from './attributes'
import { defaultUiDensity, type UiDensity } from './density'
import {
  defaultGlassThemeParameters,
  glassMeshBackground,
  type GlassThemeParameters,
} from './glassTheme'
import {
  defaultPalettePresetId,
  defaultThemeMode,
  palettePresets,
  resolvePaletteMode,
  type PalettePresetId,
  type ResolvedThemeMode,
  type ThemeMode,
} from './theme'

export type RootThemeState = {
  readonly mode?: ThemeMode
  readonly palettePreset?: PalettePresetId
  readonly density?: UiDensity
  readonly accentHue?: number
  readonly glassTheme?: GlassThemeParameters
}

export type AppliedRootTheme = {
  readonly mode: ThemeMode
  readonly resolvedMode: ResolvedThemeMode
  readonly palettePreset: PalettePresetId
  readonly density: UiDensity
  readonly accentHue: number
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

export function applyRootTheme(
  root: HTMLElement,
  state: RootThemeState,
  systemMode: ResolvedThemeMode = getSystemThemeMode(),
): AppliedRootTheme {
  const mode = state.mode ?? defaultThemeMode
  const palettePreset = state.palettePreset ?? defaultPalettePresetId
  const density = state.density ?? defaultUiDensity
  const accentHue = normalizeAccentHue(state.accentHue ?? defaultAccentHue)
  const glassTheme = state.glassTheme ?? defaultGlassThemeParameters
  const requestedMode = resolveThemeMode(mode, systemMode)
  const resolvedMode = resolvePaletteMode(palettePreset, requestedMode)
  const palette = palettePresets[palettePreset]
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
  root.setAttribute(designDataAttributes.palettePreset, palettePreset)
  root.setAttribute(designDataAttributes.paletteStyle, palette.style)
  root.setAttribute(designDataAttributes.uiDensity, density)
  root.classList.toggle(designClassNames.dark, resolvedMode === 'dark')
  root.style.setProperty('--ph-accent-hue', String(accentHue))
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
    palettePreset === 'glass' ? accentGlassSoft : accentSoft,
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
    palettePreset,
    density,
    accentHue,
    glassTheme,
  }
}
