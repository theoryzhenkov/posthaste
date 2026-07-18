import type { DesignThemePreferences } from '@/lib/design/theme/themeSettings'
import { createStore } from '@/lib/store'

import { createBroadcastChannel, parseBroadcastMessage } from './broadcast'
import {
  defaultSnapshot,
  isThemeStorageEvent,
  normalizeSnapshot,
  persistAppearancePreferences,
  readStoredClientPreferences,
  snapshotSignature,
} from './storage'
import {
  CLIENT_PREFERENCES_UPDATED,
  type ClientPreferencesBroadcastMessage,
  type ClientPreferencesListener,
  type ClientPreferencesSnapshot,
  type ClientPreferencesStore,
  type ClientPreferencesUpdater,
} from './types'

/**
 * The client-preferences store: a `createStore` (R5) over the normalized
 * snapshot, plus the cross-window machinery the appearance settings need —
 * storage events and a BroadcastChannel keep every window in agreement, and
 * both only live while someone subscribes (tenet VIII). Appearance spans
 * several legacy localStorage keys, so persistence stays in ./storage rather
 * than a single-key stored store.
 */
export function createClientPreferencesStore(): ClientPreferencesStore {
  const initial = normalizeSnapshot(readStoredClientPreferences())
  let signature = snapshotSignature(initial)
  let broadcastChannel: BroadcastChannel | null = null

  const handleStorage = (event: StorageEvent) => {
    if (
      typeof window === 'undefined' ||
      (event.storageArea !== null &&
        event.storageArea !== window.localStorage) ||
      !isThemeStorageEvent(event)
    ) {
      return
    }
    applySnapshot(readStoredClientPreferences(), {
      broadcast: false,
      persist: false,
    })
  }

  const handleBroadcast = (event: MessageEvent<unknown>) => {
    const message = parseBroadcastMessage(event.data)
    if (!message) {
      return
    }
    applySnapshot(message.snapshot, { broadcast: false, persist: false })
  }

  const store = createStore<ClientPreferencesSnapshot>(initial, {
    onActive: () => {
      if (typeof window === 'undefined') {
        return undefined
      }
      window.addEventListener('storage', handleStorage)
      broadcastChannel = createBroadcastChannel()
      broadcastChannel?.addEventListener('message', handleBroadcast)
      return () => {
        window.removeEventListener('storage', handleStorage)
        broadcastChannel?.removeEventListener('message', handleBroadcast)
        broadcastChannel?.close()
        broadcastChannel = null
      }
    },
  })

  const broadcastSnapshot = (snapshot: ClientPreferencesSnapshot) => {
    const channel = broadcastChannel ?? createBroadcastChannel()
    channel?.postMessage({
      type: CLIENT_PREFERENCES_UPDATED,
      snapshot,
    } satisfies ClientPreferencesBroadcastMessage)
    if (channel && channel !== broadcastChannel) {
      channel.close()
    }
  }

  const applySnapshot = (
    nextSnapshot: ClientPreferencesSnapshot,
    options: { broadcast: boolean; persist: boolean },
  ) => {
    const normalized = normalizeSnapshot(nextSnapshot)
    const nextSignature = snapshotSignature(normalized)
    if (nextSignature === signature) {
      return
    }
    if (options.persist) {
      persistAppearancePreferences(normalized.appearance)
    }
    signature = nextSignature
    store.set(normalized)
    if (options.broadcast) {
      broadcastSnapshot(normalized)
    }
  }

  const setAppearance = (nextAppearance: DesignThemePreferences) => {
    applySnapshot(
      { appearance: nextAppearance },
      { broadcast: true, persist: true },
    )
  }

  return {
    getSnapshot: store.get,
    getServerSnapshot: () => defaultSnapshot(),
    subscribe: (listener: ClientPreferencesListener) => {
      const unsubscribe = store.subscribe(listener)
      // Storage may have moved while no window sync was attached; catch up so
      // the first subscriber never renders a stale snapshot.
      applySnapshot(readStoredClientPreferences(), {
        broadcast: false,
        persist: false,
      })
      return unsubscribe
    },
    setAppearance,
    updateAppearance: (updater: ClientPreferencesUpdater) => {
      setAppearance(updater(store.get().appearance))
    },
  }
}
