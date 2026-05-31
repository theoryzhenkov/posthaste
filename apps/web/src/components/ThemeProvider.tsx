import {
  applyRootTheme,
  appendGlassBloom,
  designStorageKeys,
  getSystemThemeMode,
  isPalettePresetId,
  isThemeMode,
  isUiDensity,
  normalizeGlassThemeParameters,
  normalizeAccentHue,
  parseAccentHue,
  removeGlassBloom as removeGlassBloomFromTheme,
  updateGlassBloom,
  type AppliedRootTheme,
  type GlassBloomId,
  type GlassBloomPatch,
  type GlassThemeParameters,
  type PalettePresetId,
  type ThemeMode,
  type UiDensity,
} from '@/design'
import {
  defaultThemePreferences,
  type DesignThemePreferences,
} from '@/themeSettings'
import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from 'react'
import {
  DesignThemeContext,
  type DesignThemeContextValue,
} from './themeContext'

interface DesignThemeProviderProps {
  children: ReactNode
}

function storedThemeMode(): ThemeMode {
  const value = localStorage.getItem(designStorageKeys.themeMode)
  return value && isThemeMode(value) ? value : defaultThemePreferences().mode
}

function storedPalettePreset(): PalettePresetId {
  const value = localStorage.getItem(designStorageKeys.palettePreset)
  return value && isPalettePresetId(value)
    ? value
    : defaultThemePreferences().palettePreset
}

function storedDensity(): UiDensity {
  const value = localStorage.getItem(designStorageKeys.uiDensity)
  return value && isUiDensity(value) ? value : defaultThemePreferences().density
}

function storedAccentHue(): number {
  return parseAccentHue(localStorage.getItem(designStorageKeys.accentHue))
}

function storedGlassTheme(): GlassThemeParameters {
  const value = localStorage.getItem(designStorageKeys.themeParameters)
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

function readStoredThemePreferences(): DesignThemePreferences {
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

/**
 * Persist appearance preferences to localStorage. Appearance is client-local
 * presentation state (see the ownership boundary in
 * docs/eph/DESIGN-L1-deployment-modes): it never touches the daemon API.
 */
function persistThemePreferences(preferences: DesignThemePreferences) {
  if (typeof window === 'undefined') {
    return
  }
  const normalizedGlass = normalizeGlassThemeParameters(preferences.glassTheme)
  localStorage.setItem(designStorageKeys.themeMode, preferences.mode)
  localStorage.setItem(
    designStorageKeys.palettePreset,
    preferences.palettePreset,
  )
  localStorage.setItem(designStorageKeys.uiDensity, preferences.density)
  localStorage.setItem(
    designStorageKeys.accentHue,
    String(normalizeAccentHue(preferences.accentHue)),
  )
  localStorage.setItem(
    designStorageKeys.themeParameters,
    JSON.stringify({ glass: normalizedGlass }),
  )
}

export function DesignThemeProvider({ children }: DesignThemeProviderProps) {
  const [preferences, setPreferences] = useState<DesignThemePreferences>(
    readStoredThemePreferences,
  )
  const { accentHue, density, glassTheme, mode, palettePreset } = preferences

  useEffect(() => {
    persistThemePreferences(preferences)
  }, [preferences])

  const [applied, setApplied] = useState<AppliedRootTheme>(() => ({
    accentHue,
    glassTheme,
    mode,
    resolvedMode: mode === 'dark' ? 'dark' : 'light',
    palettePreset,
    density,
  }))

  useEffect(() => {
    const root = window.document.documentElement
    const apply = () =>
      setApplied(
        applyRootTheme(root, {
          mode,
          palettePreset,
          density,
          accentHue,
          glassTheme,
        }),
      )

    apply()

    if (mode !== 'system') {
      return
    }

    const query = window.matchMedia('(prefers-color-scheme: dark)')
    const handleSystemChange = () =>
      setApplied(
        applyRootTheme(
          root,
          { mode, palettePreset, density, accentHue, glassTheme },
          getSystemThemeMode(),
        ),
      )

    query.addEventListener('change', handleSystemChange)
    return () => query.removeEventListener('change', handleSystemChange)
  }, [accentHue, density, glassTheme, mode, palettePreset])

  const updatePreferences = useCallback(
    (updater: (current: DesignThemePreferences) => DesignThemePreferences) => {
      setPreferences((current) => updater(current))
    },
    [],
  )

  const setAccentHue = useCallback(
    (nextHue: number) => {
      updatePreferences((current) => ({
        ...current,
        accentHue: normalizeAccentHue(nextHue),
      }))
    },
    [updatePreferences],
  )

  const addGlassBloom = useCallback(
    (patch?: GlassBloomPatch) => {
      const result = appendGlassBloom(glassTheme, patch)
      updatePreferences((current) => ({
        ...current,
        glassTheme: result.parameters,
      }))
      return result.bloom.id
    },
    [glassTheme, updatePreferences],
  )

  const removeGlassBloom = useCallback(
    (bloomId: GlassBloomId) => {
      updatePreferences((current) => ({
        ...current,
        glassTheme: removeGlassBloomFromTheme(current.glassTheme, bloomId),
      }))
    },
    [updatePreferences],
  )

  const setGlassBloom = useCallback(
    (bloomId: GlassBloomId, patch: GlassBloomPatch) => {
      updatePreferences((current) => ({
        ...current,
        glassTheme: updateGlassBloom(current.glassTheme, bloomId, patch),
      }))
    },
    [updatePreferences],
  )

  const setMode = useCallback(
    (nextMode: ThemeMode) => {
      updatePreferences((current) => ({ ...current, mode: nextMode }))
    },
    [updatePreferences],
  )

  const setPalettePreset = useCallback(
    (nextPreset: PalettePresetId) => {
      updatePreferences((current) => ({
        ...current,
        palettePreset: nextPreset,
      }))
    },
    [updatePreferences],
  )

  const setDensity = useCallback(
    (nextDensity: UiDensity) => {
      updatePreferences((current) => ({ ...current, density: nextDensity }))
    },
    [updatePreferences],
  )

  const value = useMemo<DesignThemeContextValue>(
    () => ({
      ...applied,
      addGlassBloom,
      removeGlassBloom,
      setAccentHue,
      setGlassBloom,
      setDensity,
      setMode,
      setPalettePreset,
    }),
    [
      applied,
      addGlassBloom,
      removeGlassBloom,
      setAccentHue,
      setGlassBloom,
      setDensity,
      setMode,
      setPalettePreset,
    ],
  )

  return (
    <DesignThemeContext.Provider value={value}>
      {children}
    </DesignThemeContext.Provider>
  )
}
