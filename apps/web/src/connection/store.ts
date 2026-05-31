/**
 * The {@link ClientStore} abstraction: client-owned connection state with two
 * backends chosen by runtime.
 *
 *   - {@link WebClientStore}: `localStorage` (browser / no filesystem). Only the
 *     implicit `embedded` profile is realistic here — the injected token is the
 *     only secret and there is no secure store, so `remote`/`local-daemon`
 *     profiles (which need keyring tokens) are desktop-only.
 *   - {@link DesktopClientStore}: Tauri fs for `connections.json` in a
 *     CLIENT-owned dir plus the OS keyring for per-profile tokens, via the
 *     desktop commands.
 *
 * Both expose the same async API so the React layer is platform-agnostic. The
 * store holds NO secrets in its serialized form — tokens are fetched/saved
 * through `getToken`/`setToken` (keyring on desktop; never persisted on web).
 *
 * @spec docs/eph/DESIGN-L1-deployment-modes#client-state-layout
 */
import { isTauriRuntime } from '../desktop'
import {
  deleteProfileToken,
  getProfileToken,
  readConnectionsRaw,
  setProfileToken,
  writeConnectionsRaw,
} from './desktopBridge'
import {
  type ConnectionsFile,
  type ConnectionProfile,
  embeddedProfile,
  isConnectionsFile,
} from './types'

/** localStorage key for the web backend's connection store. */
const WEB_STORAGE_KEY = 'posthaste-connections-v1'

/**
 * A fresh store with only the implicit, auto-active embedded profile. Used as
 * the default on every backend so a fresh install behaves exactly as today.
 */
export function defaultConnectionsFile(): ConnectionsFile {
  const embedded = embeddedProfile()
  return {
    version: 1,
    activeProfileId: embedded.id,
    profiles: [embedded],
  }
}

function parseConnectionsFile(raw: string | null): ConnectionsFile {
  if (!raw) {
    return defaultConnectionsFile()
  }
  let parsed: unknown
  try {
    parsed = JSON.parse(raw)
  } catch {
    return defaultConnectionsFile()
  }
  if (!isConnectionsFile(parsed)) {
    return defaultConnectionsFile()
  }
  return parsed
}

export interface ClientStore {
  /** Load the connection-profile store (returns the default when none exists). */
  loadConnections(): Promise<ConnectionsFile>
  /** Persist the connection-profile store (never contains secrets). */
  saveConnections(file: ConnectionsFile): Promise<void>
  /** Read a per-profile token from secure storage, or `undefined`. */
  getToken(profileId: string): Promise<string | undefined>
  /** Save a per-profile token to secure storage. */
  setToken(profileId: string, token: string): Promise<void>
  /** Remove a per-profile token from secure storage (idempotent). */
  deleteToken(profileId: string): Promise<void>
  /** Whether this backend can securely hold tokens for non-embedded profiles. */
  readonly supportsSecureTokens: boolean
}

/** localStorage-backed store for the browser / web build. */
class WebClientStore implements ClientStore {
  readonly supportsSecureTokens = false

  async loadConnections(): Promise<ConnectionsFile> {
    if (typeof localStorage === 'undefined') {
      return defaultConnectionsFile()
    }
    return parseConnectionsFile(localStorage.getItem(WEB_STORAGE_KEY))
  }

  async saveConnections(file: ConnectionsFile): Promise<void> {
    if (typeof localStorage === 'undefined') {
      return
    }
    localStorage.setItem(WEB_STORAGE_KEY, JSON.stringify(file))
  }

  // The web build has no secure store. Only the embedded profile exists, whose
  // token is injected at runtime — never persisted here.
  async getToken(): Promise<string | undefined> {
    return undefined
  }

  async setToken(): Promise<void> {
    // Intentionally a no-op: refuse to persist secrets in localStorage.
  }

  async deleteToken(): Promise<void> {
    // No-op: nothing was stored.
  }
}

/** Tauri fs + OS keyring backed store for the desktop build. */
class DesktopClientStore implements ClientStore {
  readonly supportsSecureTokens = true

  async loadConnections(): Promise<ConnectionsFile> {
    return parseConnectionsFile(await readConnectionsRaw())
  }

  async saveConnections(file: ConnectionsFile): Promise<void> {
    await writeConnectionsRaw(JSON.stringify(file, null, 2))
  }

  async getToken(profileId: string): Promise<string | undefined> {
    return (await getProfileToken(profileId)) ?? undefined
  }

  async setToken(profileId: string, token: string): Promise<void> {
    await setProfileToken(profileId, token)
  }

  async deleteToken(profileId: string): Promise<void> {
    await deleteProfileToken(profileId)
  }
}

let cachedStore: ClientStore | undefined

/** The process-wide {@link ClientStore}, selected by runtime on first use. */
export function clientStore(): ClientStore {
  if (!cachedStore) {
    cachedStore = isTauriRuntime()
      ? new DesktopClientStore()
      : new WebClientStore()
  }
  return cachedStore
}

/** Test-only: reset the cached store so a fresh backend is selected. */
export function resetClientStoreForTesting(store?: ClientStore): void {
  cachedStore = store
}

/** Convenience: look up a profile by id within a loaded file. */
export function findProfile(
  file: ConnectionsFile,
  id: string | null,
): ConnectionProfile | undefined {
  if (id === null) {
    return undefined
  }
  return file.profiles.find((profile) => profile.id === id)
}
