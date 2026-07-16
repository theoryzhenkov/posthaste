/**
 * Thin wrappers over the desktop (Tauri) commands that back the client-owned
 * connection state: the `connections.json` store, the per-profile token keyring,
 * and `daemon.json` discovery. These are only invoked when `isTauriRuntime()`
 * is true; the web backend never touches them.
 *
 * @spec docs/eph/DESIGN-L1-deployment-modes#client-state-layout
 */
import { invoke } from '@tauri-apps/api/core'

/** Read the raw `connections.json` contents, or `null` on a fresh install. */
export async function readConnectionsRaw(): Promise<string | null> {
  return (await invoke<string | null>('client_connections_read')) ?? null
}

/** Persist the `connections.json` contents (no secrets — tokens are separate). */
export async function writeConnectionsRaw(contents: string): Promise<void> {
  await invoke('client_connections_write', { contents })
}

/** Read a per-profile remote token from the OS keyring, or `null` if absent. */
export async function getProfileToken(
  profileId: string,
): Promise<string | null> {
  return (
    (await invoke<string | null>('client_token_get', { profileId })) ?? null
  )
}

/** Store (or replace) a per-profile remote token in the OS keyring. */
export async function setProfileToken(
  profileId: string,
  token: string,
): Promise<void> {
  await invoke('client_token_set', { profileId, token })
}

/** Delete a per-profile remote token from the OS keyring (idempotent). */
export async function deleteProfileToken(profileId: string): Promise<void> {
  await invoke('client_token_delete', { profileId })
}

/** A locally-running daemon discovered via `<STATE_ROOT>/daemon.json`. */
export interface LocalDaemon {
  port: number
  token: string
}

/** Discover a locally-running daemon, or `null` when none is running. */
export async function readLocalDaemon(): Promise<LocalDaemon | null> {
  return (await invoke<LocalDaemon | null>('client_local_daemon_read')) ?? null
}
