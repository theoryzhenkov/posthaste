import {
  normalizeGlassThemeParameters,
  parseThemeMode,
  parseUiDensity,
} from '@/lib/design'
import type { DesignThemePreferences } from '@/lib/design/theme/themeSettings'

import { normalizeAppearancePreferences } from './storage'
import {
  CLIENT_PREFERENCES_CHANNEL,
  CLIENT_PREFERENCES_UPDATED,
  type ClientPreferencesBroadcastMessage,
} from './types'

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null
}

function parseAppearancePreferences(
  value: unknown,
): DesignThemePreferences | null {
  if (!isRecord(value)) {
    return null
  }
  const { density, glassTheme, light, dark, mode, theme } = value
  const parsedDensity =
    typeof density === 'string' ? parseUiDensity(density) : null
  const parsedMode = typeof mode === 'string' ? parseThemeMode(mode) : null
  if (
    !parsedDensity ||
    !parsedMode ||
    typeof theme !== 'string' ||
    !isRecord(light) ||
    !isRecord(dark)
  ) {
    return null
  }
  return normalizeAppearancePreferences({
    density: parsedDensity,
    glassTheme: normalizeGlassThemeParameters(glassTheme),
    light: light as unknown as DesignThemePreferences['light'],
    dark: dark as unknown as DesignThemePreferences['dark'],
    mode: parsedMode,
    theme,
  })
}

export function parseBroadcastMessage(
  value: unknown,
): ClientPreferencesBroadcastMessage | null {
  if (!isRecord(value) || value.type !== CLIENT_PREFERENCES_UPDATED) {
    return null
  }
  const snapshot = isRecord(value.snapshot) ? value.snapshot : null
  const appearance = snapshot
    ? parseAppearancePreferences(snapshot.appearance)
    : null
  if (!appearance) {
    return null
  }
  return {
    type: CLIENT_PREFERENCES_UPDATED,
    snapshot: { appearance },
  }
}

export function createBroadcastChannel(): BroadcastChannel | null {
  if (
    typeof window === 'undefined' ||
    typeof BroadcastChannel === 'undefined'
  ) {
    return null
  }
  return new BroadcastChannel(CLIENT_PREFERENCES_CHANNEL)
}
