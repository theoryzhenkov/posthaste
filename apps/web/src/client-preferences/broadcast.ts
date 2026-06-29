import {
  isThemeMode,
  isUiDensity,
  normalizeGlassThemeParameters,
} from '@/design'
import type { DesignThemePreferences } from '@/themeSettings'

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
  if (
    typeof density !== 'string' ||
    !isUiDensity(density) ||
    typeof mode !== 'string' ||
    !isThemeMode(mode) ||
    typeof theme !== 'string' ||
    !isRecord(light) ||
    !isRecord(dark)
  ) {
    return null
  }
  return normalizeAppearancePreferences({
    density,
    glassTheme: normalizeGlassThemeParameters(glassTheme),
    light: light as unknown as DesignThemePreferences['light'],
    dark: dark as unknown as DesignThemePreferences['dark'],
    mode,
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
