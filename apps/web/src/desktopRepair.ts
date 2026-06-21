/**
 * Trigger a local-database repair and relaunch.
 *
 * Writes the repair marker via the desktop command, then relaunches: on the next
 * launch the embedded server quarantines the corrupt `mail.sqlite` and rebuilds
 * it from config. Accounts and passwords (config + keychain) are unaffected; the
 * local mail cache re-syncs. Desktop-only — in the browser build the user must
 * remove the database file manually.
 */
import { isTauriRuntime } from './desktop'

export function canRepairLocalDatabase(): boolean {
  return isTauriRuntime()
}

export async function repairLocalDatabaseAndRestart(): Promise<void> {
  if (!isTauriRuntime()) {
    return
  }
  const { invoke } = await import('@tauri-apps/api/core')
  await invoke('request_database_repair')
  const { relaunch } = await import('@tauri-apps/plugin-process')
  await relaunch()
}
