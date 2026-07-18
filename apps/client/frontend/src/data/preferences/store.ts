import type { DesignThemePreferences } from '@/lib/design/theme/themeSettings'

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

class LocalClientPreferencesStore implements ClientPreferencesStore {
  private broadcastChannel: BroadcastChannel | null = null
  private readonly listeners = new Set<ClientPreferencesListener>()
  private snapshot = normalizeSnapshot(readStoredClientPreferences())
  private signature = snapshotSignature(this.snapshot)

  readonly getSnapshot = () => this.snapshot

  readonly getServerSnapshot = () => defaultSnapshot()

  readonly subscribe = (listener: ClientPreferencesListener) => {
    this.listeners.add(listener)
    if (this.listeners.size === 1) {
      this.startCrossWindowSync()
    }
    this.applySnapshot(readStoredClientPreferences(), {
      broadcast: false,
      persist: false,
    })

    return () => {
      this.listeners.delete(listener)
      if (this.listeners.size === 0) {
        this.stopCrossWindowSync()
      }
    }
  }

  readonly setAppearance = (nextAppearance: DesignThemePreferences) => {
    this.applySnapshot(
      { appearance: nextAppearance },
      { broadcast: true, persist: true },
    )
  }

  readonly updateAppearance = (updater: ClientPreferencesUpdater) => {
    this.setAppearance(updater(this.snapshot.appearance))
  }

  private readonly handleStorage = (event: StorageEvent) => {
    if (
      typeof window === 'undefined' ||
      (event.storageArea !== null &&
        event.storageArea !== window.localStorage) ||
      !isThemeStorageEvent(event)
    ) {
      return
    }
    this.applySnapshot(readStoredClientPreferences(), {
      broadcast: false,
      persist: false,
    })
  }

  private readonly handleBroadcast = (event: MessageEvent<unknown>) => {
    const message = parseBroadcastMessage(event.data)
    if (!message) {
      return
    }
    this.applySnapshot(message.snapshot, { broadcast: false, persist: false })
  }

  private startCrossWindowSync() {
    if (typeof window === 'undefined') {
      return
    }
    window.addEventListener('storage', this.handleStorage)
    this.broadcastChannel = createBroadcastChannel()
    this.broadcastChannel?.addEventListener('message', this.handleBroadcast)
  }

  private stopCrossWindowSync() {
    if (typeof window !== 'undefined') {
      window.removeEventListener('storage', this.handleStorage)
    }
    this.broadcastChannel?.removeEventListener('message', this.handleBroadcast)
    this.broadcastChannel?.close()
    this.broadcastChannel = null
  }

  private applySnapshot(
    nextSnapshot: ClientPreferencesSnapshot,
    options: { broadcast: boolean; persist: boolean },
  ) {
    const normalized = normalizeSnapshot(nextSnapshot)
    const nextSignature = snapshotSignature(normalized)
    if (nextSignature === this.signature) {
      return
    }
    if (options.persist) {
      persistAppearancePreferences(normalized.appearance)
    }
    this.snapshot = normalized
    this.signature = nextSignature
    this.emit()
    if (options.broadcast) {
      this.broadcastSnapshot(normalized)
    }
  }

  private broadcastSnapshot(snapshot: ClientPreferencesSnapshot) {
    const channel = this.broadcastChannel ?? createBroadcastChannel()
    channel?.postMessage({
      type: CLIENT_PREFERENCES_UPDATED,
      snapshot,
    } satisfies ClientPreferencesBroadcastMessage)
    if (channel && channel !== this.broadcastChannel) {
      channel.close()
    }
  }

  private emit() {
    for (const listener of this.listeners) {
      listener()
    }
  }
}

export function createClientPreferencesStore(): ClientPreferencesStore {
  return new LocalClientPreferencesStore()
}
