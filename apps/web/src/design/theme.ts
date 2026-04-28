export const themeModes = ['light', 'dark', 'system'] as const
export const resolvedThemeModes = ['light', 'dark'] as const

export type ThemeMode = (typeof themeModes)[number]
export type ResolvedThemeMode = (typeof resolvedThemeModes)[number]

export const defaultThemeMode = 'dark' as const satisfies ThemeMode

export const palettePresetIds = [
  'neutral',
  'paperInk',
  'brutalist',
  'glass',
  'acid',
  'marzipan',
  'botanical',
] as const

export type PalettePresetId = (typeof palettePresetIds)[number]

export type PalettePresetStyle =
  | 'neutral'
  | 'editorial'
  | 'brutalist'
  | 'glass'
  | 'acid'
  | 'marzipan'
  | 'botanical'

export type PalettePreset = {
  readonly id: PalettePresetId
  readonly label: string
  readonly description: string
  readonly modes: readonly ResolvedThemeMode[]
  readonly style: PalettePresetStyle
}

export const palettePresets = {
  neutral: {
    id: 'neutral',
    label: 'Neutral',
    description: 'Cool gray, balanced contrast',
    modes: ['light', 'dark'],
    style: 'neutral',
  },
  paperInk: {
    id: 'paperInk',
    label: 'Paper & Ink',
    description: 'Bright white, thin rules, editorial serifs, ink-red accents',
    modes: ['light'],
    style: 'editorial',
  },
  brutalist: {
    id: 'brutalist',
    label: 'Brutalist',
    description: 'Monospace everywhere, 2px borders, zero rounding',
    modes: ['light', 'dark'],
    style: 'brutalist',
  },
  glass: {
    id: 'glass',
    label: 'Arc Glass',
    description: 'Layered translucent panes over a vivid desktop wash',
    modes: ['dark', 'light'],
    style: 'glass',
  },
  acid: {
    id: 'acid',
    label: 'Acid',
    description: 'Pure black + electric lime, mechanical precision',
    modes: ['dark'],
    style: 'acid',
  },
  marzipan: {
    id: 'marzipan',
    label: 'Marzipan',
    description: 'Soft pastels, generous rounding, friendly',
    modes: ['light'],
    style: 'marzipan',
  },
  botanical: {
    id: 'botanical',
    label: 'Botanical',
    description: 'Deep forest green on cream, quiet and confident',
    modes: ['light'],
    style: 'botanical',
  },
} as const satisfies Record<PalettePresetId, PalettePreset>

export const defaultPalettePresetId =
  'neutral' as const satisfies PalettePresetId

export function isThemeMode(value: string): value is ThemeMode {
  return themeModes.includes(value as ThemeMode)
}

export function isPalettePresetId(value: string): value is PalettePresetId {
  return palettePresetIds.includes(value as PalettePresetId)
}

export function resolvePaletteMode(
  presetId: PalettePresetId,
  mode: ResolvedThemeMode,
): ResolvedThemeMode {
  const preset = palettePresets[presetId]
  const supportedModes: readonly ResolvedThemeMode[] = preset.modes
  return supportedModes.includes(mode) ? mode : preset.modes[0]
}
