import {
  designStorageKeys,
  isPalettePresetId,
  isThemeMode,
  isUiDensity,
  normalizeAccentHue,
  normalizeGlassThemeParameters,
  parseAccentHue,
} from '@/design'
import {
  defaultThemePreferences,
  type DesignThemePreferences,
} from '@/themeSettings'

import type { ClientPreferencesSnapshot } from './types'

const DEFAULT_CLIENT_PREFERENCES_SNAPSHOT: ClientPreferencesSnapshot = {
  appearance: defaultThemePreferences(),
}

export function defaultSnapshot(): ClientPreferencesSnapshot {
  return DEFAULT_CLIENT_PREFERENCES_SNAPSHOT
}

function storedThemeMode(): DesignThemePreferences['mode'] {
  const value = window.localStorage.getItem(designStorageKeys.themeMode)
  return value && isThemeMode(value) ? value : defaultThemePreferences().mode
}

function storedPalettePreset(): DesignThemePreferences['palettePreset'] {
  const value = window.localStorage.getItem(designStorageKeys.palettePreset)
  return value && isPalettePresetId(value)
    ? value
    : defaultThemePreferences().palettePreset
}

function storedDensity(): DesignThemePreferences['density'] {
  const value = window.localStorage.getItem(designStorageKeys.uiDensity)
  return value && isUiDensity(value) ? value : defaultThemePreferences().density
}

function storedAccentHue(): number {
  return parseAccentHue(
    window.localStorage.getItem(designStorageKeys.accentHue),
  )
}

function storedGlassTheme(): DesignThemePreferences['glassTheme'] {
  const value = window.localStorage.getItem(designStorageKeys.themeParameters)
  if (!value) {
    return normalizeGlassThemeParameters(null)
  }
  try {
    const parsed = JSON.parse(value) as Record<string, unknown>
    return normalizeGlassThemeParameters(parsed.glass)
  } catch {
    return normalizeGlassThemeParameters(null)
  }
}

function readStoredAppearancePreferences(): DesignThemePreferences {
  if (typeof window === 'undefined') {
    return defaultThemePreferences()
  }

  return {
    accentHue: storedAccentHue(),
    glassTheme: storedGlassTheme(),
    mode: storedThemeMode(),
    palettePreset: storedPalettePreset(),
    density: storedDensity(),
  }
}

export function readStoredClientPreferences(): ClientPreferencesSnapshot {
  return { appearance: readStoredAppearancePreferences() }
}

export function normalizeAppearancePreferences(
  preferences: DesignThemePreferences,
): DesignThemePreferences {
  return {
    accentHue: normalizeAccentHue(preferences.accentHue),
    glassTheme: normalizeGlassThemeParameters(preferences.glassTheme),
    mode: preferences.mode,
    palettePreset: preferences.palettePreset,
    density: preferences.density,
  }
}

export function normalizeSnapshot(
  snapshot: ClientPreferencesSnapshot,
): ClientPreferencesSnapshot {
  return { appearance: normalizeAppearancePreferences(snapshot.appearance) }
}

export function snapshotSignature(snapshot: ClientPreferencesSnapshot): string {
  return JSON.stringify(normalizeSnapshot(snapshot))
}

export function persistAppearancePreferences(
  preferences: DesignThemePreferences,
) {
  if (typeof window === 'undefined') {
    return
  }
  const normalized = normalizeAppearancePreferences(preferences)
  window.localStorage.setItem(designStorageKeys.themeMode, normalized.mode)
  window.localStorage.setItem(
    designStorageKeys.palettePreset,
    normalized.palettePreset,
  )
  window.localStorage.setItem(designStorageKeys.uiDensity, normalized.density)
  window.localStorage.setItem(
    designStorageKeys.accentHue,
    String(normalized.accentHue),
  )
  window.localStorage.setItem(
    designStorageKeys.themeParameters,
    JSON.stringify({ glass: normalized.glassTheme }),
  )
}

const themeStorageKeys = new Set<string>(Object.values(designStorageKeys))

export function isThemeStorageEvent(event: StorageEvent): boolean {
  return event.key === null || themeStorageKeys.has(event.key)
}

/**
 * One-time import guard for migrating the renderer's localStorage appearance
 * cache into TOML (`[appearance]` in app.toml). Set after the cache is first
 * written to TOML so the import is not repeated.
 *
 * @spec docs/eph/DESIGN-L2-appearance-toml
 */
const APPEARANCE_IMPORTED_FLAG = 'posthaste.appearance.imported'

export function hasImportedAppearance(): boolean {
  return (
    typeof window !== 'undefined' &&
    window.localStorage.getItem(APPEARANCE_IMPORTED_FLAG) === '1'
  )
}

export function markAppearanceImported(): void {
  if (typeof window === 'undefined') {
    return
  }
  window.localStorage.setItem(APPEARANCE_IMPORTED_FLAG, '1')
}
