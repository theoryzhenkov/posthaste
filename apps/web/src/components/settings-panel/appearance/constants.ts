import { Monitor, Moon, Sun } from 'lucide-react'

import type { PalettePresetId, ThemeMode, UiDensity } from '@/design'

export const themeModeLabels = {
  dark: 'Dark',
  light: 'Light',
  system: 'System',
} as const satisfies Record<ThemeMode, string>

export const themeModeIcons = {
  dark: Moon,
  light: Sun,
  system: Monitor,
} as const satisfies Record<ThemeMode, typeof Moon>

export const densityLabels = {
  compact: 'Compact',
  cozy: 'Cozy',
  comfortable: 'Comfortable',
} as const satisfies Record<UiDensity, string>

export const paletteSwatches = {
  neutral: [
    'oklch(0.22 0.008 60)',
    'oklch(0.34 0.06 250)',
    'oklch(0.68 0.17 45)',
  ],
  paperInk: [
    'oklch(0.985 0.005 80)',
    'oklch(0.20 0.01 60)',
    'oklch(0.62 0.14 25)',
  ],
  brutalist: ['oklch(0.98 0 0)', 'oklch(0.12 0 0)', 'oklch(0.68 0.17 45)'],
  glass: [
    'oklch(0.27 0.10 286)',
    'oklch(0.67 0.14 318)',
    'oklch(0.72 0.13 205)',
  ],
  acid: ['oklch(0.10 0 0)', 'oklch(0.82 0.25 135)', 'oklch(0.96 0.01 125)'],
  marzipan: [
    'oklch(0.96 0.035 75)',
    'oklch(0.84 0.08 35)',
    'oklch(0.72 0.11 320)',
  ],
  botanical: [
    'oklch(0.25 0.055 150)',
    'oklch(0.92 0.035 115)',
    'oklch(0.68 0.08 145)',
  ],
} as const satisfies Record<PalettePresetId, readonly [string, string, string]>

export const hueGradient =
  'linear-gradient(90deg, oklch(0.68 0.17 0), oklch(0.68 0.17 45), oklch(0.68 0.17 90), oklch(0.68 0.17 145), oklch(0.68 0.17 205), oklch(0.68 0.17 260), oklch(0.68 0.17 315), oklch(0.68 0.17 360))'
