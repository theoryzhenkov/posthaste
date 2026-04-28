import {
  applyRootTheme,
  appendGlassBloom,
  defaultAccentHue,
  defaultPalettePresetId,
  defaultThemeMode,
  defaultUiDensity,
  designStorageKeys,
  getSystemThemeMode,
  normalizeGlassThemeParameters,
  isPalettePresetId,
  isThemeMode,
  isUiDensity,
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

interface DesignThemePreferences {
  accentHue: number
  density: UiDensity
  glassTheme: GlassThemeParameters
  mode: ThemeMode
  palettePreset: PalettePresetId
}

function storedThemeMode(): ThemeMode {
  const value = localStorage.getItem(designStorageKeys.themeMode)
  return value && isThemeMode(value) ? value : defaultThemeMode
}

function storedPalettePreset(): PalettePresetId {
  const value = localStorage.getItem(designStorageKeys.palettePreset)
  return value && isPalettePresetId(value) ? value : defaultPalettePresetId
}

function storedDensity(): UiDensity {
  const value = localStorage.getItem(designStorageKeys.uiDensity)
  return value && isUiDensity(value) ? value : defaultUiDensity
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

function readInitialThemeState(): DesignThemePreferences {
  if (typeof window === 'undefined') {
    return {
      accentHue: defaultAccentHue,
      glassTheme: normalizeGlassThemeParameters(null),
      mode: defaultThemeMode,
      palettePreset: defaultPalettePresetId,
      density: defaultUiDensity,
    }
  }

  return {
    accentHue: storedAccentHue(),
    glassTheme: storedGlassTheme(),
    mode: storedThemeMode(),
    palettePreset: storedPalettePreset(),
    density: storedDensity(),
  }
}

export function DesignThemeProvider({ children }: DesignThemeProviderProps) {
  const [preferences, setPreferences] = useState(readInitialThemeState)
  const { accentHue, density, glassTheme, mode, palettePreset } = preferences
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

  const persistGlassTheme = useCallback(
    (nextGlassTheme: GlassThemeParameters) => {
      let currentParameters: Record<string, unknown> = {}
      const stored = localStorage.getItem(designStorageKeys.themeParameters)
      if (stored) {
        try {
          currentParameters = JSON.parse(stored) as Record<string, unknown>
        } catch {
          currentParameters = {}
        }
      }
      localStorage.setItem(
        designStorageKeys.themeParameters,
        JSON.stringify({ ...currentParameters, glass: nextGlassTheme }),
      )
    },
    [],
  )

  const setAccentHue = useCallback((nextHue: number) => {
    const hue = normalizeAccentHue(nextHue)
    localStorage.setItem(designStorageKeys.accentHue, String(hue))
    setPreferences((current) => ({ ...current, accentHue: hue }))
  }, [])

  const addGlassBloom = useCallback(
    (patch?: GlassBloomPatch) => {
      const result = appendGlassBloom(glassTheme, patch)
      persistGlassTheme(result.parameters)
      setPreferences((current) => ({
        ...current,
        glassTheme: result.parameters,
      }))
      return result.bloom.id
    },
    [glassTheme, persistGlassTheme],
  )

  const removeGlassBloom = useCallback(
    (bloomId: GlassBloomId) => {
      setPreferences((current) => {
        const nextGlassTheme = removeGlassBloomFromTheme(
          current.glassTheme,
          bloomId,
        )
        persistGlassTheme(nextGlassTheme)
        return { ...current, glassTheme: nextGlassTheme }
      })
    },
    [persistGlassTheme],
  )

  const setGlassBloom = useCallback(
    (bloomId: GlassBloomId, patch: GlassBloomPatch) => {
      setPreferences((current) => {
        const nextGlassTheme = updateGlassBloom(
          current.glassTheme,
          bloomId,
          patch,
        )
        persistGlassTheme(nextGlassTheme)
        return { ...current, glassTheme: nextGlassTheme }
      })
    },
    [persistGlassTheme],
  )

  const setMode = useCallback((nextMode: ThemeMode) => {
    localStorage.setItem(designStorageKeys.themeMode, nextMode)
    setPreferences((current) => ({ ...current, mode: nextMode }))
  }, [])

  const setPalettePreset = useCallback((nextPreset: PalettePresetId) => {
    localStorage.setItem(designStorageKeys.palettePreset, nextPreset)
    setPreferences((current) => ({ ...current, palettePreset: nextPreset }))
  }, [])

  const setDensity = useCallback((nextDensity: UiDensity) => {
    localStorage.setItem(designStorageKeys.uiDensity, nextDensity)
    setPreferences((current) => ({ ...current, density: nextDensity }))
  }, [])

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
