/**
 * Connection-profile model (Phase B of the deployment-modes design).
 *
 * A client owns one or more connection profiles describing *which daemon* it
 * talks to. The active profile is resolved into a concrete {@link ResolvedConnection}
 * (base URL + token + optional host header) that `api/client.ts` reads on every
 * request — replacing the module-load-frozen `BASE_URL`/`AUTH_TOKEN`.
 *
 * Profiles never carry secrets: remote tokens live in the OS keyring (desktop)
 * keyed by profile id; the embedded profile's token is injected at runtime.
 *
 * @spec docs/eph/DESIGN-L1-deployment-modes#connection-profiles
 */

/**
 * How a profile resolves its backend:
 *   - `embedded`:     the in-process server injected as `__POSTHASTE_PORT__`/
 *                     `__POSTHASTE_TOKEN__` (today's bundled behavior).
 *   - `local-daemon`: a daemon running on this machine, discovered via the
 *                     daemon's `<STATE_ROOT>/daemon.json` (desktop only).
 *   - `remote`:       an explicit `baseUrl` (e.g. over Tailscale) with the token
 *                     held in the OS keyring (desktop only).
 */
export type ConnectionMode = 'embedded' | 'local-daemon' | 'remote'

export interface ConnectionProfile {
  /** Stable id; also the keyring account for a `remote` profile's token. */
  id: string
  /** Human-readable label shown in the connection UI. */
  name: string
  /**
   * Base URL including the `/v1` prefix for `remote` profiles, e.g.
   * `http://daemon.tailnet:3001/v1`. Ignored for `embedded`/`local-daemon`,
   * which derive `http://127.0.0.1:<port>/v1` from injection/`daemon.json`.
   */
  baseUrl?: string
  /**
   * Optional `Host` header value sent for a `remote` profile, the client side
   * of the daemon's future `allowed_hosts` allowlist. Loopback profiles imply
   * `127.0.0.1` and never set this.
   */
  hostHeader?: string
  mode: ConnectionMode
  /**
   * Pointer to the keyring entry holding this profile's token. The secret is
   * NEVER stored inline. Defaults to the profile id when present.
   */
  tokenRef?: string
}

export interface ConnectionsFile {
  version: 1
  /** Id of the profile currently active, or `null` when none is selected. */
  activeProfileId: string | null
  profiles: ConnectionProfile[]
}

/**
 * A resolved, ready-to-use connection. `token` is omitted when the server does
 * not require auth (or none was resolvable). `hostHeader` is set only for
 * `remote` profiles that pin one.
 */
export interface ResolvedConnection {
  baseUrl: string
  token?: string
  hostHeader?: string
}

/** The stable id of the implicit, auto-active embedded profile. */
export const EMBEDDED_PROFILE_ID = 'embedded'

/** The implicit embedded profile, auto-active on a fresh install. */
export function embeddedProfile(): ConnectionProfile {
  return { id: EMBEDDED_PROFILE_ID, name: 'This computer', mode: 'embedded' }
}

/** Type guard for a parsed-from-disk {@link ConnectionsFile} (version-tolerant). */
export function isConnectionsFile(value: unknown): value is ConnectionsFile {
  if (typeof value !== 'object' || value === null) {
    return false
  }
  const obj = value as Record<string, unknown>
  return (
    Object.keys(obj).every(isKnownConnectionsFileField) &&
    obj.version === 1 &&
    Array.isArray(obj.profiles) &&
    obj.profiles.every(isConnectionProfile) &&
    (obj.activeProfileId === null || typeof obj.activeProfileId === 'string')
  )
}

function isKnownConnectionsFileField(key: string): boolean {
  return key === 'version' || key === 'activeProfileId' || key === 'profiles'
}

function isConnectionProfile(value: unknown): value is ConnectionProfile {
  if (typeof value !== 'object' || value === null) {
    return false
  }
  const obj = value as Record<string, unknown>
  if (!Object.keys(obj).every(isKnownConnectionProfileField)) {
    return false
  }
  if (
    typeof obj.id !== 'string' ||
    typeof obj.name !== 'string' ||
    !isConnectionMode(obj.mode)
  ) {
    return false
  }
  if (
    obj.baseUrl !== undefined &&
    (typeof obj.baseUrl !== 'string' || !isSafeProfileBaseUrl(obj.baseUrl))
  ) {
    return false
  }
  if (
    obj.hostHeader !== undefined &&
    (typeof obj.hostHeader !== 'string' ||
      !isSafeProfileHostHeader(obj.hostHeader))
  ) {
    return false
  }
  if (obj.tokenRef !== undefined && typeof obj.tokenRef !== 'string') {
    return false
  }
  return !Object.keys(obj).some(isInlineSecretField)
}

function percentDecodeRepeated(value: string): {
  decoded: string
  stable: boolean
} {
  let current = value
  for (let pass = 0; pass < 8; pass += 1) {
    let decoded: string
    try {
      decoded = decodeURIComponent(current)
    } catch {
      return { decoded: current, stable: false }
    }
    if (decoded === current) {
      return { decoded, stable: true }
    }
    current = decoded
  }
  return { decoded: current, stable: false }
}

function containsSecretMarker(value: string): boolean {
  const { decoded, stable } = percentDecodeRepeated(value)
  if (!stable) {
    return true
  }
  const normalized = decoded.replace(/[^a-z0-9]/gi, '')
  return /(token|secret|password|credential|authorization|authheader|bearer|apikey|privatekey)/i.test(
    normalized,
  )
}

function isInlineSecretField(key: string): boolean {
  return key !== 'tokenRef' && containsSecretMarker(key)
}

function containsUrlPathSecretMarker(value: string): boolean {
  const { decoded, stable } = percentDecodeRepeated(value)
  if (!stable) {
    return true
  }
  return decoded
    .split(/[\/;]/)
    .map((segment) => segment.replace(/[^a-z0-9]/gi, '').toLowerCase())
    .some(
      (segment) =>
        segment === 'token' ||
        segment === 'secret' ||
        segment.includes('token') ||
        segment.includes('secret') ||
        segment.includes('authorization') ||
        segment.includes('authheader') ||
        segment.includes('bearer') ||
        segment.includes('apikey') ||
        segment.includes('privatekey') ||
        segment.includes('password') ||
        segment.includes('credential'),
    )
}

function isSafeProfileBaseUrl(value: string): boolean {
  let parsed: URL
  try {
    parsed = new URL(value)
  } catch {
    return false
  }
  if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') {
    return false
  }
  if (
    parsed.username ||
    parsed.password ||
    parsed.search ||
    parsed.hash ||
    containsUrlPathSecretMarker(parsed.pathname)
  ) {
    return false
  }
  return true
}

function isSafeProfileHostHeader(value: string): boolean {
  if (!value || /[\u0000-\u001f\u007f]/.test(value)) {
    return false
  }
  let parsed: URL
  try {
    parsed = new URL(`http://${value}`)
  } catch {
    return false
  }
  return (
    parsed.hostname.length > 0 &&
    !parsed.username &&
    !parsed.password &&
    parsed.pathname === '/' &&
    !parsed.search &&
    !parsed.hash
  )
}

function isKnownConnectionProfileField(key: string): boolean {
  return (
    key === 'id' ||
    key === 'name' ||
    key === 'baseUrl' ||
    key === 'hostHeader' ||
    key === 'mode' ||
    key === 'tokenRef'
  )
}

function isConnectionMode(value: unknown): value is ConnectionMode {
  return value === 'embedded' || value === 'local-daemon' || value === 'remote'
}
