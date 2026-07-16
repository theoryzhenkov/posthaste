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
  type ResolvedThemeMode,
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
  useRef,
  useState,
  useSyncExternalStore,
  type ReactNode,
} from 'react'
import { toast } from 'sonner'
import {
  DesignThemeContext,
  type DesignThemeContextValue,
} from './themeContext'

/**
 * Debounce window for the write-through appearance PATCH. A hue-slider drag
 * fires many `onChange` ticks; this coalesces them into one trailing PATCH with
 * the final value (the local cache updates instantly on every tick regardless).
 */
const APPEARANCE_PATCH_DEBOUNCE_MS = 250

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
  const { density, glassTheme, light, dark, mode, theme } = preferences

  // Coalesce write-through PATCHes: a continuous gesture (dragging a hue slider
  // fires onChange per tick) would otherwise storm `PATCH /v1/settings` with a
  // burst of concurrent config writes. We apply each change to the cache
  // immediately (instant local recolor) but debounce the network PATCH to one
  // trailing call with the final value; the rollback baseline is the state
  // before the burst started.
  const patchTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const patchBaselineRef = useRef<DesignThemePreferences | null>(null)

  const flushAppearancePatch = useCallback(() => {
    if (patchTimerRef.current) {
      clearTimeout(patchTimerRef.current)
      patchTimerRef.current = null
    }
    const baseline = patchBaselineRef.current
    if (baseline === null) {
      return
    }
    patchBaselineRef.current = null
    const latest = store.getSnapshot().appearance
    void runtimeMutations.settings
      .patch({ appearance: designToWireAppearance(latest) })
      .then((updated) => {
        queryClient.setQueryData(queryKeys.settings, updated)
      })
      .catch(() => {
        store.setAppearance(baseline)
        toast.error('Appearance change could not be saved.')
      })
  }, [store])

  // Flush any pending appearance PATCH on unmount so a change made just before
  // the settings panel closes still persists to TOML.
  useEffect(() => flushAppearancePatch, [flushAppearancePatch])

  const [applied, setApplied] = useState<AppliedRootTheme>(() => ({
    mode,
    resolvedMode: mode === 'dark' ? 'dark' : 'light',
    theme,
    density,
    light,
    dark,
    glassTheme,
  }))

  useEffect(() => {
    const root = window.document.documentElement
    const state = { mode, theme, density, light, dark, glassTheme }
    const apply = () => setApplied(applyRootTheme(root, state))

    apply()

    if (mode !== 'system') {
      return
    }

    const query = window.matchMedia('(prefers-color-scheme: dark)')
    const handleSystemChange = () =>
      setApplied(applyRootTheme(root, state, getSystemThemeMode()))

    query.addEventListener('change', handleSystemChange)
    return () => query.removeEventListener('change', handleSystemChange)
  }, [density, glassTheme, light, dark, mode, theme])

  const updatePreferences = useCallback(
    (updater: (current: DesignThemePreferences) => DesignThemePreferences) => {
      if (!writeThrough) {
        store.updateAppearance(updater)
        return
      }
      // Write-through (Option A): optimistically apply to the localStorage
      // cache (instant UI, no flash) + PATCH TOML (source of truth), debounced
      // (see `flushAppearancePatch`). Roll back to the burst baseline on
      // failure so the cache never diverges from what persisted.
      const current = store.getSnapshot().appearance
      if (patchBaselineRef.current === null) {
        patchBaselineRef.current = current
      }
      store.setAppearance(updater(current))
      if (patchTimerRef.current) {
        clearTimeout(patchTimerRef.current)
      }
      patchTimerRef.current = setTimeout(
        flushAppearancePatch,
        APPEARANCE_PATCH_DEBOUNCE_MS,
      )
    },
    [store, writeThrough, flushAppearancePatch],
  )

  const setAccentHue = useCallback(
    (targetMode: ResolvedThemeMode, nextHue: number) => {
      updatePreferences((current) => ({
        ...current,
        [targetMode]: {
          ...current[targetMode],
          accentHue: normalizeAccentHue(nextHue),
        },
      }))
    },
    [updatePreferences],
  )

  const setSurfaceHue = useCallback(
    (targetMode: ResolvedThemeMode, nextHue: number) => {
      updatePreferences((current) => ({
        ...current,
        [targetMode]: {
          ...current[targetMode],
          surfaceHue: normalizeAccentHue(nextHue),
        },
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

  const setTheme = useCallback(
    (nextTheme: string) => {
      updatePreferences((current) => ({
        ...current,
        theme: nextTheme,
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
      setSurfaceHue,
      setGlassBloom,
      setDensity,
      setMode,
      setTheme,
    }),
    [
      applied,
      addGlassBloom,
      removeGlassBloom,
      setAccentHue,
      setSurfaceHue,
      setGlassBloom,
      setDensity,
      setMode,
      setTheme,
    ],
  )

  return (
    <DesignThemeContext.Provider value={value}>
      {children}
    </DesignThemeContext.Provider>
  )
}
