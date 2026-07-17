import type { DesignThemePreferences } from '@/themeSettings'

export interface ClientPreferencesSnapshot {
  appearance: DesignThemePreferences
}

export type ClientPreferencesListener = () => void

export type ClientPreferencesUpdater = (
  current: DesignThemePreferences,
) => DesignThemePreferences

export interface ClientPreferencesStore {
  getSnapshot: () => ClientPreferencesSnapshot
  getServerSnapshot: () => ClientPreferencesSnapshot
  subscribe: (listener: ClientPreferencesListener) => () => void
  setAppearance: (nextAppearance: DesignThemePreferences) => void
  updateAppearance: (updater: ClientPreferencesUpdater) => void
}

export const CLIENT_PREFERENCES_CHANNEL = 'posthaste.clientPreferences.v1'
export const CLIENT_PREFERENCES_UPDATED = 'client-preferences-updated'

export interface ClientPreferencesBroadcastMessage {
  type: typeof CLIENT_PREFERENCES_UPDATED
  snapshot: ClientPreferencesSnapshot
}
