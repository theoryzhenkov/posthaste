/**
 * The process-wide *active connection* that `api/client.ts` reads on every
 * request. This is the dynamic replacement for the old module-load
 * `const BASE_URL`/`const AUTH_TOKEN`.
 *
 * Behavior-preservation guarantee: the holder is seeded *synchronously* at
 * module load with the embedded resolution — i.e. the injected
 * `__POSTHASTE_PORT__`/`__POSTHASTE_TOKEN__` (or the browser-dev fallback),
 * exactly what the old consts computed. So even before any async profile
 * resolution runs, `client.ts` sees the same baseUrl/token as today. Async
 * resolution (`initActiveConnection`) and runtime switches
 * (`applyResolvedConnection`) only re-point the holder; they never change the
 * default.
 *
 * @spec docs/eph/DESIGN-L1-deployment-modes#connection-profiles
 */
import { injectedBaseUrl, injectedToken } from './injected'
import { type ResolvedConnection } from './types'

/**
 * The synchronous embedded default: identical to the legacy
 * `resolveBaseUrl()`/`resolveAuthToken()` consts. Seeds the holder so the very
 * first API call in the bundled build is unchanged.
 */
function embeddedDefault(): ResolvedConnection {
  return {
    baseUrl: injectedBaseUrl(),
    token: injectedToken(),
  }
}

let active: ResolvedConnection = embeddedDefault()
let activeProfileId: string | null = null

const listeners = new Set<() => void>()

function notify(): void {
  for (const listener of listeners) {
    listener()
  }
}

/** The current active connection (always defined; seeded to embedded default). */
export function getActiveConnection(): ResolvedConnection {
  return active
}

/** Test-only: restore the active connection to the embedded runtime default. */
export function resetActiveConnectionForTesting(): void {
  active = embeddedDefault()
  activeProfileId = null
  notify()
}

/** The id of the profile backing the active connection, if known. */
export function getActiveProfileId(): string | null {
  return activeProfileId
}

/**
 * Re-point the active connection (e.g. after resolving a profile or switching).
 * Notifies listeners so react-query can refetch against the new daemon.
 */
export function applyResolvedConnection(
  connection: ResolvedConnection,
  profileId: string | null,
): void {
  active = connection
  activeProfileId = profileId
  notify()
}

/**
 * Subscribe to active-connection changes. Returns an unsubscribe function.
 * Used by the React provider to invalidate queries on a profile switch.
 */
export function subscribeActiveConnection(listener: () => void): () => void {
  listeners.add(listener)
  return () => listeners.delete(listener)
}
