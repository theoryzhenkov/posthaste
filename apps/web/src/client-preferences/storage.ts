import {
  designStorageKeys,
  isThemeMode,
  isUiDensity,
  normalizeAccentHue,
  normalizeGlassThemeParameters,
} from '@/design'
import {
  defaultThemeColors,
  defaultThemePreferences,
  type DesignThemePreferences,
  type ThemeColors,
} from '@/themeSettings'

import type { ClientPreferencesSnapshot } from './types'

const DEFAULT_CLIENT_PREFERENCES_SNAPSHOT: ClientPreferencesSnapshot = {
  appearance: defaultThemePreferences(),
}

export function defaultSnapshot(): ClientPreferencesSnapshot {
  return DEFAULT_CLIENT_PREFERENCES_SNAPSHOT
}

function normalizeThemeColors(value: unknown): ThemeColors {
  const fallback = defaultThemeColors()
  const record =
    value && typeof value === 'object' ? (value as Record<string, unknown>) : {}
  return {
    accentHue: normalizeAccentHue(
      typeof record.accentHue === 'number'
        ? record.accentHue
        : fallback.accentHue,
    ),
    surfaceHue: normalizeAccentHue(
      typeof record.surfaceHue === 'number'
        ? record.surfaceHue
        : fallback.surfaceHue,
    ),
  }
}

function storedThemeMode(): DesignThemePreferences['mode'] {
  const value = window.localStorage.getItem(designStorageKeys.themeMode)
  return value && isThemeMode(value) ? value : defaultThemePreferences().mode
}

function storedTheme(): string {
  const value = window.localStorage.getItem(designStorageKeys.theme)
  return value && value.trim() ? value : defaultThemePreferences().theme
}

function storedDensity(): DesignThemePreferences['density'] {
  const value = window.localStorage.getItem(designStorageKeys.uiDensity)
  return value && isUiDensity(value) ? value : defaultThemePreferences().density
}

function storedColors(): Pick<DesignThemePreferences, 'light' | 'dark'> {
  const value = window.localStorage.getItem(designStorageKeys.themeColors)
  if (!value) {
    return { light: defaultThemeColors(), dark: defaultThemeColors() }
  }
  try {
    const parsed = JSON.parse(value) as Record<string, unknown>
    return {
      light: normalizeThemeColors(parsed.light),
      dark: normalizeThemeColors(parsed.dark),
    }
  } catch {
    return { light: defaultThemeColors(), dark: defaultThemeColors() }
  }
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

  const colors = storedColors()
  return {
    mode: storedThemeMode(),
    theme: storedTheme(),
    density: storedDensity(),
    light: colors.light,
    dark: colors.dark,
    glassTheme: storedGlassTheme(),
  }
}

export function readStoredClientPreferences(): ClientPreferencesSnapshot {
  return { appearance: readStoredAppearancePreferences() }
}

export function normalizeAppearancePreferences(
  preferences: DesignThemePreferences,
): DesignThemePreferences {
  return {
    mode: preferences.mode,
    theme: preferences.theme,
    density: preferences.density,
    light: normalizeThemeColors(preferences.light),
    dark: normalizeThemeColors(preferences.dark),
    glassTheme: normalizeGlassThemeParameters(preferences.glassTheme),
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
  window.localStorage.setItem(designStorageKeys.theme, normalized.theme)
  window.localStorage.setItem(designStorageKeys.uiDensity, normalized.density)
  window.localStorage.setItem(
    designStorageKeys.themeColors,
    JSON.stringify({ light: normalized.light, dark: normalized.dark }),
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
