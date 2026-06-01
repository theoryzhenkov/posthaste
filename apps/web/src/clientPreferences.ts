import {
  designStorageKeys,
  isPalettePresetId,
  isThemeMode,
  isUiDensity,
  normalizeAccentHue,
  normalizeGlassThemeParameters,
  parseAccentHue,
} from '@/design'
import {
  defaultThemePreferences,
  type DesignThemePreferences,
} from '@/themeSettings'

export interface ClientPreferencesSnapshot {
  appearance: DesignThemePreferences
}

type ClientPreferencesListener = () => void

type ClientPreferencesUpdater = (
  current: DesignThemePreferences,
) => DesignThemePreferences

export interface ClientPreferencesStore {
  getSnapshot: () => ClientPreferencesSnapshot
  getServerSnapshot: () => ClientPreferencesSnapshot
  subscribe: (listener: ClientPreferencesListener) => () => void
  setAppearance: (nextAppearance: DesignThemePreferences) => void
  updateAppearance: (updater: ClientPreferencesUpdater) => void
}

const CLIENT_PREFERENCES_CHANNEL = 'posthaste.clientPreferences.v1'
const CLIENT_PREFERENCES_UPDATED = 'client-preferences-updated'

interface ClientPreferencesBroadcastMessage {
  type: typeof CLIENT_PREFERENCES_UPDATED
  snapshot: ClientPreferencesSnapshot
}

const DEFAULT_CLIENT_PREFERENCES_SNAPSHOT: ClientPreferencesSnapshot = {
  appearance: defaultThemePreferences(),
}

function defaultSnapshot(): ClientPreferencesSnapshot {
  return DEFAULT_CLIENT_PREFERENCES_SNAPSHOT
}

function storedThemeMode(): DesignThemePreferences['mode'] {
  const value = window.localStorage.getItem(designStorageKeys.themeMode)
  return value && isThemeMode(value) ? value : defaultThemePreferences().mode
}

function storedPalettePreset(): DesignThemePreferences['palettePreset'] {
  const value = window.localStorage.getItem(designStorageKeys.palettePreset)
  return value && isPalettePresetId(value)
    ? value
    : defaultThemePreferences().palettePreset
}

function storedDensity(): DesignThemePreferences['density'] {
  const value = window.localStorage.getItem(designStorageKeys.uiDensity)
  return value && isUiDensity(value) ? value : defaultThemePreferences().density
}

function storedAccentHue(): number {
  return parseAccentHue(
    window.localStorage.getItem(designStorageKeys.accentHue),
  )
}

function storedGlassTheme(): DesignThemePreferences['glassTheme'] {
  const value = window.localStorage.getItem(designStorageKeys.themeParameters)
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

function readStoredAppearancePreferences(): DesignThemePreferences {
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

function readStoredClientPreferences(): ClientPreferencesSnapshot {
  return { appearance: readStoredAppearancePreferences() }
}

function normalizeAppearancePreferences(
  preferences: DesignThemePreferences,
): DesignThemePreferences {
  return {
    accentHue: normalizeAccentHue(preferences.accentHue),
    glassTheme: normalizeGlassThemeParameters(preferences.glassTheme),
    mode: preferences.mode,
    palettePreset: preferences.palettePreset,
    density: preferences.density,
  }
}

function normalizeSnapshot(
  snapshot: ClientPreferencesSnapshot,
): ClientPreferencesSnapshot {
  return { appearance: normalizeAppearancePreferences(snapshot.appearance) }
}

function snapshotSignature(snapshot: ClientPreferencesSnapshot): string {
  return JSON.stringify(normalizeSnapshot(snapshot))
}

function persistAppearancePreferences(preferences: DesignThemePreferences) {
  if (typeof window === 'undefined') {
    return
  }
  const normalized = normalizeAppearancePreferences(preferences)
  window.localStorage.setItem(designStorageKeys.themeMode, normalized.mode)
  window.localStorage.setItem(
    designStorageKeys.palettePreset,
    normalized.palettePreset,
  )
  window.localStorage.setItem(designStorageKeys.uiDensity, normalized.density)
  window.localStorage.setItem(
    designStorageKeys.accentHue,
    String(normalized.accentHue),
  )
  window.localStorage.setItem(
    designStorageKeys.themeParameters,
    JSON.stringify({ glass: normalized.glassTheme }),
  )
}

const themeStorageKeys = new Set<string>(Object.values(designStorageKeys))

function isThemeStorageEvent(event: StorageEvent): boolean {
  return event.key === null || themeStorageKeys.has(event.key)
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null
}

function parseAppearancePreferences(
  value: unknown,
): DesignThemePreferences | null {
  if (!isRecord(value)) {
    return null
  }
  const { accentHue, density, glassTheme, mode, palettePreset } = value
  if (
    typeof accentHue !== 'number' ||
    typeof density !== 'string' ||
    !isUiDensity(density) ||
    typeof mode !== 'string' ||
    !isThemeMode(mode) ||
    typeof palettePreset !== 'string' ||
    !isPalettePresetId(palettePreset)
  ) {
    return null
  }
  return normalizeAppearancePreferences({
    accentHue,
    density,
    glassTheme: normalizeGlassThemeParameters(glassTheme),
    mode,
    palettePreset,
  })
}

function parseBroadcastMessage(
  value: unknown,
): ClientPreferencesBroadcastMessage | null {
  if (!isRecord(value) || value.type !== CLIENT_PREFERENCES_UPDATED) {
    return null
  }
  const snapshot = isRecord(value.snapshot) ? value.snapshot : null
  const appearance = snapshot
    ? parseAppearancePreferences(snapshot.appearance)
    : null
  if (!appearance) {
    return null
  }
  return {
    type: CLIENT_PREFERENCES_UPDATED,
    snapshot: { appearance },
  }
}

function createBroadcastChannel(): BroadcastChannel | null {
  if (
    typeof window === 'undefined' ||
    typeof BroadcastChannel === 'undefined'
  ) {
    return null
  }
  return new BroadcastChannel(CLIENT_PREFERENCES_CHANNEL)
}

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

export const clientPreferencesStore: ClientPreferencesStore =
  new LocalClientPreferencesStore()
