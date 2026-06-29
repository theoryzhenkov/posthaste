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
import { queryClient } from '@/app/queryClient'
import {
  clientPreferencesStore,
  type ClientPreferencesStore,
} from '@/clientPreferences'
import { designToWireAppearance } from '@/client-preferences/wireMapping'
import type { DesignThemePreferences } from '@/themeSettings'
import { queryKeys } from '@/queryKeys'
import { runtimeMutations } from '@/runtime/mutations'
import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  useSyncExternalStore,
  type ReactNode,
} from 'react'
import { toast } from 'sonner'
import {
  DesignThemeContext,
  type DesignThemeContextValue,
} from './themeContext'

interface DesignThemeProviderProps {
  children: ReactNode
  /**
   * Client-preferences source. Defaults to the process-wide singleton; inject a
   * fresh instance (via `createClientPreferencesStore`) to isolate tests from
   * shared cross-window-sync state.
   */
  store?: ClientPreferencesStore
  /**
   * Write appearance changes through to TOML (PATCH /v1/settings) — the single
   * source of truth. Off in tests (which exercise the localStorage cache in
   * isolation); on in the app via the AppearanceSettingsSync bridge.
   *
   * @spec docs/eph/DESIGN-L2-appearance-toml
   */
  writeThrough?: boolean
}

export function DesignThemeProvider({
  children,
  store = clientPreferencesStore,
  writeThrough = false,
}: DesignThemeProviderProps) {
  const { appearance: preferences } = useSyncExternalStore(
    store.subscribe,
    store.getSnapshot,
    store.getServerSnapshot,
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
      if (!writeThrough) {
        store.updateAppearance(updater)
        return
      }
      // Write-through (Option A): optimistically apply to the localStorage
      // cache (instant UI, no flash) + PATCH TOML (source of truth). Roll back
      // on failure so the cache never diverges from what persisted.
      const current = store.getSnapshot().appearance
      const next = updater(current)
      store.setAppearance(next)
      void runtimeMutations.settings
        .patch({ appearance: designToWireAppearance(next) })
        .then((updated) => {
          queryClient.setQueryData(queryKeys.settings, updated)
        })
        .catch(() => {
          store.setAppearance(current)
          toast.error('Appearance change could not be saved.')
        })
    },
    [store, writeThrough],
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
