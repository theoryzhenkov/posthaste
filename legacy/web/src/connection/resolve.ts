/**
 * Resolve the active connection profile into a concrete {@link ResolvedConnection}.
 *
 * The three modes mirror the design and the MCP adapter's env→daemon.json→baseUrl
 * pattern:
 *   - `embedded`:     injected `__POSTHASTE_PORT__`/`__POSTHASTE_TOKEN__` (the
 *                     default active profile → identical to today's behavior).
 *   - `local-daemon`: read `<STATE_ROOT>/daemon.json` via the desktop bridge.
 *   - `remote`:       the profile's `baseUrl` + keyring token + optional Host.
 *
 * When no profile is active or it cannot be resolved, the result is a
 * `NeedsConnection` state rather than a throw, so the UI can show a connect
 * screen instead of firing API calls at a non-existent backend.
 *
 * @spec docs/eph/DESIGN-L1-deployment-modes#connection-profiles
 */
import { isTauriRuntime } from '../desktop'
import { readLocalDaemon } from './desktopBridge'
import { injectedBaseUrl, injectedPort, injectedToken } from './injected'
import { clientStore, defaultConnectionsFile, findProfile } from './store'
import { type ConnectionProfile, type ResolvedConnection } from './types'

export type ConnectionResolution =
  | { status: 'connected'; connection: ResolvedConnection; profileId: string }
  | { status: 'needs-connection'; reason: string }

function normalizeApiBaseUrl(baseUrl: string): string {
  return baseUrl.replace(/\/+$/, '')
}

/**
 * Resolve a single profile. Exposed so the connection UI can validate a profile
 * before activating it.
 */
export async function resolveProfile(
  profile: ConnectionProfile,
): Promise<ConnectionResolution> {
  switch (profile.mode) {
    case 'embedded':
      // Client-only desktop build: no embedded server was injected, so the
      // embedded profile is meaningless — surface a connect prompt instead of
      // firing API calls at a non-existent loopback backend. In the bundled
      // build the port is injected; in browser dev the `VITE_API_BASE_URL`
      // fallback is legitimate (not a Tauri runtime).
      if (isTauriRuntime() && injectedPort() === undefined) {
        return {
          status: 'needs-connection',
          reason:
            'This build has no embedded server. Connect to a daemon to continue.',
        }
      }
      return {
        status: 'connected',
        profileId: profile.id,
        connection: {
          baseUrl: injectedBaseUrl(),
          token: injectedToken(),
        },
      }

    case 'local-daemon': {
      if (!isTauriRuntime()) {
        return {
          status: 'needs-connection',
          reason: 'A local daemon profile requires the desktop app.',
        }
      }
      const daemon = await readLocalDaemon()
      if (!daemon) {
        return {
          status: 'needs-connection',
          reason:
            'No local daemon is running. Start it with `posthaste serve`, or pick another connection.',
        }
      }
      return {
        status: 'connected',
        profileId: profile.id,
        connection: {
          baseUrl: `http://127.0.0.1:${daemon.port}/v1`,
          token: daemon.token,
        },
      }
    }

    case 'remote': {
      if (!isTauriRuntime()) {
        return {
          status: 'needs-connection',
          reason: 'Remote connections require the desktop app.',
        }
      }
      if (!profile.baseUrl) {
        return {
          status: 'needs-connection',
          reason: `Profile "${profile.name}" is missing a base URL.`,
        }
      }
      const token = await clientStore().getToken(profile.tokenRef ?? profile.id)
      return {
        status: 'connected',
        profileId: profile.id,
        connection: {
          baseUrl: normalizeApiBaseUrl(profile.baseUrl),
          token: token ?? undefined,
          hostHeader: profile.hostHeader,
        },
      }
    }
  }
}

/**
 * Resolve the currently-active connection from the store. On a fresh install
 * (or browser build) the default store has the embedded profile auto-active, so
 * this returns the same baseUrl/token the old module-load consts produced.
 */
export async function resolveActiveConnection(): Promise<ConnectionResolution> {
  const store = clientStore()
  let file
  try {
    file = await store.loadConnections()
  } catch {
    file = defaultConnectionsFile()
  }

  const profile = findProfile(file, file.activeProfileId)
  if (!profile) {
    return {
      status: 'needs-connection',
      reason: 'No connection is selected. Add a daemon to connect to.',
    }
  }
  return resolveProfile(profile)
}
