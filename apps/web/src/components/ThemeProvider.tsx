import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
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
import { fetchSettings, patchSettings } from '@/api/client'
import type { AppSettings } from '@/api/types'
import { queryKeys } from '@/queryKeys'
import {
  appAppearanceFromPreferences,
  defaultThemePreferences,
  preferencesFromAppAppearance,
  shouldMigrateStoredThemePreferences,
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

const themeMigrationStorageKey = 'posthaste.themeMigratedToAppSettings.v1'

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

function hasCompletedThemeMigration() {
  return localStorage.getItem(themeMigrationStorageKey) === 'complete'
}

export function DesignThemeProvider({ children }: DesignThemeProviderProps) {
  const queryClient = useQueryClient()
  const [initialStoredPreferences] = useState(readStoredThemePreferences)
  const [optimisticPreferences, setOptimisticPreferences] =
    useState<DesignThemePreferences | null>(null)

  const settingsQuery = useQuery({
    queryKey: queryKeys.settings,
    queryFn: fetchSettings,
  })
  const { mutate: saveAppearance } = useMutation({
    mutationFn: (appearance: ReturnType<typeof appAppearanceFromPreferences>) =>
      patchSettings({ appearance }),
    onSuccess: (settings) => {
      queryClient.setQueryData<AppSettings>(queryKeys.settings, settings)
      setOptimisticPreferences(null)
    },
  })

  const serverAppearance = settingsQuery.data?.appearance
  const shouldMigrate = Boolean(
    serverAppearance &&
    !hasCompletedThemeMigration() &&
    shouldMigrateStoredThemePreferences(
      serverAppearance,
      initialStoredPreferences,
    ),
  )
  const serverPreferences = serverAppearance
    ? preferencesFromAppAppearance(serverAppearance)
    : null
  const preferences =
    optimisticPreferences ??
    (shouldMigrate
      ? initialStoredPreferences
      : (serverPreferences ?? initialStoredPreferences))
  const { accentHue, density, glassTheme, mode, palettePreset } = preferences

  useEffect(() => {
    if (!serverAppearance || hasCompletedThemeMigration()) {
      return
    }

    if (shouldMigrate) {
      saveAppearance(appAppearanceFromPreferences(initialStoredPreferences), {
        onSuccess: () =>
          localStorage.setItem(themeMigrationStorageKey, 'complete'),
      })
      return
    }

    localStorage.setItem(themeMigrationStorageKey, 'complete')
  }, [
    initialStoredPreferences,
    saveAppearance,
    serverAppearance,
    shouldMigrate,
  ])

  useEffect(() => {
    if (!optimisticPreferences) {
      return
    }

    const timeout = window.setTimeout(() => {
      saveAppearance(appAppearanceFromPreferences(optimisticPreferences))
    }, 300)
    return () => window.clearTimeout(timeout)
  }, [optimisticPreferences, saveAppearance])

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
      setOptimisticPreferences((current) => updater(current ?? preferences))
    },
    [preferences],
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
