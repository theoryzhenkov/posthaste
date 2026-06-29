import { useQuery } from '@tanstack/react-query'
import { useEffect, useRef } from 'react'

import {
  clientPreferencesStore,
  type ClientPreferencesStore,
} from '@/clientPreferences'
import {
  appearanceSignature,
  designToWireAppearance,
  isDefaultAppearance,
  wireAppearanceToDesign,
} from '@/client-preferences/wireMapping'
import {
  hasImportedAppearance,
  markAppearanceImported,
} from '@/client-preferences/storage'
import { queryKeys } from '@/queryKeys'
import { runtimeMutations } from '@/runtime/mutations'
import { runtimeViews } from '@/runtime/views'

/**
 * Reconciles the renderer's localStorage appearance cache with the TOML source
 * of truth (`[appearance]` in `app.toml`) once at boot — Option A of the
 * appearance migration.
 *
 * - TOML wins if it differs from the cache (so an LLM/CLI edit to `app.toml`
 *   takes effect on next launch), without a boot flash: the cache still boots
 *   the theme synchronously, then this reconciles after the settings fetch.
 * - If TOML is unset and the cache holds user customization, import it to TOML
 *   once (guarded by a localStorage flag).
 *
 * Runs exactly once (first settings arrival). After boot, appearance writes are
 * write-through (the ThemeProvider setters PATCH), so the cache and TOML stay in
 * sync without re-reconciling — which would risk reverting an optimistic write
 * if settings were refetched mid-PATCH. Live reload of external app.toml edits is
 * P1.3 (deferred).
 *
 * @spec docs/eph/DESIGN-L2-appearance-toml
 */
export function useAppearanceSettingsSync(
  store: ClientPreferencesStore = clientPreferencesStore,
) {
  const { data: settings } = useQuery({
    queryKey: queryKeys.settings,
    queryFn: runtimeViews.settings.current,
  })
  const reconciledRef = useRef(false)

  useEffect(() => {
    if (!settings || reconciledRef.current) {
      return
    }
    reconciledRef.current = true

    const cache = store.getSnapshot().appearance
    const toml = settings.appearance ?? null

    if (toml) {
      const fromToml = wireAppearanceToDesign(toml)
      if (appearanceSignature(fromToml) !== appearanceSignature(cache)) {
        // TOML wins — an external app.toml edit overrides the cache.
        store.setAppearance(fromToml)
      }
      return
    }

    // TOML is unset: one-time import the cache to TOML if it has user
    // customization and we haven't imported yet.
    if (isDefaultAppearance(cache)) {
      return
    }
    if (hasImportedAppearance()) {
      return
    }
    void runtimeMutations.settings
      .patch({ appearance: designToWireAppearance(cache) })
      .then(() => markAppearanceImported())
      .catch(() => {
        // Non-fatal: retry on next boot. The cache remains the working state.
      })
  }, [settings, store])
}

/** Mounts the appearance↔TOML reconciliation. Render inside DesignThemeProvider. */
export function AppearanceSettingsSync() {
  useAppearanceSettingsSync()
  return null
}
