/**
 * Repair local state and relaunch.
 *
 * The client holds no durable mail store — every rendered fact is a backend
 * query answer — so repair is a backend affair: write the server DB repair
 * marker (on the next launch the embedded server quarantines a corrupt
 * `mail.sqlite` and rebuilds it from config), then relaunch.
 *
 * Accounts and passwords (config + keychain) are unaffected and mail
 * re-syncs. Desktop-only — the browser build cannot relaunch.
 */
import { isTauriRuntime } from '@/lib/platform/runtime'
import { clearClientLocalState, markRepairRequested } from './feedback'

export async function repairLocalDatabaseAndRestart(): Promise<void> {
  if (!isTauriRuntime()) {
    return
  }
  const { invoke } = await import('@tauri-apps/api/core')
  await invoke('request_database_repair')
  // Breadcrumb so the next boot can confirm the repair to the user (it would
  // otherwise look like nothing happened). Survives the relaunch via localStorage.
  markRepairRequested('repair')
  const { relaunch } = await import('@tauri-apps/plugin-process')
  await relaunch()
}

export function canFactoryReset(): boolean {
  return isTauriRuntime()
}

/**
 * Full factory reset + relaunch: clears every local store (client UI state,
 * and — via the desktop marker consumed before the server starts — the mail
 * database, config, accounts, and connection store). Unlike repair this
 * removes accounts; the user starts from a clean install. Mail on the server
 * is untouched. Desktop-only.
 */
export async function factoryResetAndRestart(): Promise<void> {
  if (!isTauriRuntime()) {
    return
  }
  clearClientLocalState()
  const { invoke } = await import('@tauri-apps/api/core')
  await invoke('request_factory_reset')
  // Set the breadcrumb AFTER clearing local state so it survives to the toast.
  markRepairRequested('factory-reset')
  const { relaunch } = await import('@tauri-apps/plugin-process')
  await relaunch()
}
