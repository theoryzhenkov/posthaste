import {
  applyRootTheme,
  appendGlassBloom,
  getSystemThemeMode,
  normalizeAccentHue,
  removeGlassBloom as removeGlassBloomFromTheme,
  updateGlassBloom,
  type AppliedRootTheme,
  type GlassBloomId,
  type GlassBloomPatch,
  type PalettePresetId,
  type ThemeMode,
  type UiDensity,
} from '@/design'
import { clientPreferencesStore } from '@/clientPreferences'
import type { DesignThemePreferences } from '@/themeSettings'
import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  useSyncExternalStore,
  type ReactNode,
} from 'react'
import {
  DesignThemeContext,
  type DesignThemeContextValue,
} from './themeContext'

interface DesignThemeProviderProps {
  children: ReactNode
}

export function DesignThemeProvider({ children }: DesignThemeProviderProps) {
  const { appearance: preferences } = useSyncExternalStore(
    clientPreferencesStore.subscribe,
    clientPreferencesStore.getSnapshot,
    clientPreferencesStore.getServerSnapshot,
  )
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

  const updatePreferences = useCallback(
    (updater: (current: DesignThemePreferences) => DesignThemePreferences) => {
      clientPreferencesStore.updateAppearance(updater)
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
