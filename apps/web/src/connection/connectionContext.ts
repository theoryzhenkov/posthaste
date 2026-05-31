/**
 * Context, types, and hook for the active connection. Kept separate from the
 * {@link ActiveConnectionProvider} component (in `useActiveConnection.tsx`) so
 * the provider file only exports a component (react-refresh constraint).
 *
 * @spec docs/eph/DESIGN-L1-deployment-modes#connection-profiles
 */
import { createContext, useContext } from 'react'

import { resolveProfile } from './resolve'
import { type ConnectionMode, type ConnectionProfile } from './types'

/** Input for adding a profile through the connection UI. */
export interface AddProfileInput {
  name: string
  mode: ConnectionMode
  baseUrl?: string
  hostHeader?: string
  /** Remote token, stored in the keyring (never persisted in connections.json). */
  token?: string
}

export type ActiveConnectionStatus =
  | 'loading'
  | 'connected'
  | 'needs-connection'

export interface ActiveConnectionContextValue {
  status: ActiveConnectionStatus
  /** Reason text when `status === 'needs-connection'`. */
  reason: string | null
  profiles: ConnectionProfile[]
  activeProfileId: string | null
  /** Add a profile (and its keyring token when supplied) and select it. */
  addProfile(input: AddProfileInput): Promise<void>
  /** Switch the active profile and re-resolve the connection. */
  selectProfile(id: string): Promise<void>
  /** Remove a profile and its keyring token; reselect a remaining one. */
  removeProfile(id: string): Promise<void>
  /** Re-resolve the active profile (e.g. after a daemon starts). */
  refresh(): Promise<void>
  /** Whether the active backend can securely hold non-embedded tokens. */
  supportsSecureTokens: boolean
}

export const ActiveConnectionContext =
  createContext<ActiveConnectionContextValue | null>(null)

export function useActiveConnection(): ActiveConnectionContextValue {
  const value = useContext(ActiveConnectionContext)
  if (!value) {
    throw new Error(
      'useActiveConnection must be used within an ActiveConnectionProvider',
    )
  }
  return value
}

/** Validate a prospective profile without persisting it (used by the UI). */
export async function probeProfile(
  profile: ConnectionProfile,
): Promise<{ ok: boolean; reason?: string }> {
  const resolution = await resolveProfile(profile)
  return resolution.status === 'connected'
    ? { ok: true }
    : { ok: false, reason: resolution.reason }
}
