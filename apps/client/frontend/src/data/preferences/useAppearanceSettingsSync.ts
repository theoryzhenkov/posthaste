import { useQueryClient } from '@tanstack/react-query'
import { useEffect, useRef } from 'react'

import {
  clientPreferencesStore,
  type ClientPreferencesStore,
} from '@/data/preferences'
import {
  appearanceSignature,
  isDefaultAppearance,
  wireAppearanceToDesign,
} from '@/data/preferences/wireMapping'
import { persistAppearance } from '@/data/preferences/persistAppearance'
import {
  hasImportedAppearance,
  markAppearanceImported,
} from '@/data/preferences/storage'
import { useAppSettings, useMailClient } from '@/data'

/**
 * Reconciles the renderer's localStorage appearance cache with the settings
 * document (the `appSettings` family, backed by `[appearance]` in `app.toml`)
 * once at boot.
 *
 * - The settings document wins if it differs from the cache (so an LLM/CLI
 *   edit to `app.toml` takes effect on next launch), without a boot flash:
 *   the cache still boots the theme synchronously, then this reconciles after
 *   the settings answer arrives.
 * - If the document has no appearance and the cache holds user customization,
 *   import it once through `updateSettings` (guarded by a localStorage flag).
 *
 * Runs exactly once (first settings arrival). After boot, appearance writes
 * are write-through (the theme setters persist via `updateSettings`), so the
 * cache and the document stay in sync without re-reconciling.
 */
export function useAppearanceSettingsSync(
  store: ClientPreferencesStore = clientPreferencesStore,
) {
  const client = useMailClient()
  const queryClient = useQueryClient()
  const { data } = useAppSettings()
  const settings = data?.settings
  const reconciledRef = useRef(false)

  useEffect(() => {
    if (!settings || reconciledRef.current) {
      return
    }
    reconciledRef.current = true

    const cache = store.getSnapshot().appearance
    const stored = settings.appearance ?? null

    if (stored) {
      const fromStored = wireAppearanceToDesign(stored)
      if (appearanceSignature(fromStored) !== appearanceSignature(cache)) {
        // The settings document wins — an external app.toml edit overrides
        // the cache.
        store.setAppearance(fromStored)
      }
      return
    }

    // The document has no appearance: one-time import the cache if it has
    // user customization and we haven't imported yet.
    if (isDefaultAppearance(cache)) {
      return
    }
    if (hasImportedAppearance()) {
      return
    }
    void persistAppearance(client, queryClient, cache)
      .then(() => markAppearanceImported())
      .catch(() => {
        // Non-fatal: retry on next boot. The cache remains the working state.
      })
  }, [settings, store, client, queryClient])
}

/** Mounts the appearance reconciliation. Render inside DesignThemeProvider. */
export function AppearanceSettingsSync() {
  useAppearanceSettingsSync()
  return null
}
