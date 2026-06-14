import {
  isPalettePresetId,
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
  const { accentHue, density, glassTheme, mode, palettePreset } = value
  if (
    typeof accentHue !== 'number' ||
    typeof density !== 'string' ||
    !isUiDensity(density) ||
    typeof mode !== 'string' ||
    !isThemeMode(mode) ||
    typeof palettePreset !== 'string' ||
    !isPalettePresetId(palettePreset)
  ) {
    return null
  }
  return normalizeAppearancePreferences({
    accentHue,
    density,
    glassTheme: normalizeGlassThemeParameters(glassTheme),
    mode,
    palettePreset,
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
