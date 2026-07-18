import { Monitor, Moon, Sun } from 'lucide-react'

import type { BuiltInThemeId, ThemeMode, UiDensity } from '@/lib/design'

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

export const themeSwatches = {
  neutral: [
    'oklch(0.22 0.008 60)',
    'oklch(0.34 0.06 250)',
    'oklch(0.68 0.17 45)',
  ],
  glass: [
    'oklch(0.27 0.10 286)',
    'oklch(0.67 0.14 318)',
    'oklch(0.72 0.13 205)',
  ],
} as const satisfies Record<BuiltInThemeId, readonly [string, string, string]>

export const hueGradient =
  'linear-gradient(90deg, oklch(0.68 0.17 0), oklch(0.68 0.17 45), oklch(0.68 0.17 90), oklch(0.68 0.17 145), oklch(0.68 0.17 205), oklch(0.68 0.17 260), oklch(0.68 0.17 315), oklch(0.68 0.17 360))'
