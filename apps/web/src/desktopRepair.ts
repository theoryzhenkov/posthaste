/**
 * Repair local state and relaunch — a composite of BOTH durable client stores.
 *
 * 1. Reset the IndexedDB replica (pending set + undo history). This is the store the
 *    reactive mail-list views are computed from; a wedged replica is the real
 *    cause of "views stuck loading forever", and rebuilding `mail.sqlite` alone
 *    never clears it (the prior "repair does nothing" bug).
 * 2. Write the server DB repair marker: on the next launch the embedded server
 *    quarantines a corrupt `mail.sqlite` and rebuilds it from config.
 * 3. Relaunch into the fresh replica + rebuilt DB.
 *
 * Accounts and passwords (config + keychain) are unaffected and mail re-syncs;
 * the only loss is never-dispatched pending-set mutations. Desktop-only — the browser
 * build cannot relaunch.
 */
import { isTauriRuntime } from './desktop'
import { resetReplicaDatabase } from './runtime/replica/replicaDatabase'
import { clearClientLocalState, markRepairRequested } from './repairFeedback'
import { LOG_EVENTS } from './logEvents'
import { syncLogger } from './logger'

export function canRepairLocalDatabase(): boolean {
  return isTauriRuntime()
}

export async function repairLocalDatabaseAndRestart(): Promise<void> {
  if (!isTauriRuntime()) {
    return
  }
  // Clear the client replica first. Best-effort: a failure here must not block
  // the server-side rebuild + relaunch, which on its own still fixes the
  // mail.sqlite-corruption class.
  try {
    await resetReplicaDatabase()
  } catch (error) {
    syncLogger.warn(
      { event: LOG_EVENTS.databaseRepairFailed, error },
      'replica reset failed during repair; continuing with the server rebuild',
    )
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
 * Full factory reset + relaunch: clears EVERY local store (the IndexedDB replica,
 * all client UI state, and — via the desktop marker consumed before the server
 * starts — the mail database, config, accounts, and connection store). Unlike
 * repair this removes accounts; the user starts from a clean install. Mail on the
 * server is untouched. Desktop-only.
 */
export async function factoryResetAndRestart(): Promise<void> {
  if (!isTauriRuntime()) {
    return
  }
  try {
    await resetReplicaDatabase()
  } catch (error) {
    syncLogger.warn(
      { event: LOG_EVENTS.databaseRepairFailed, error },
      'replica reset failed during factory reset; continuing with the daemon wipe',
    )
  }
  clearClientLocalState()
  const { invoke } = await import('@tauri-apps/api/core')
  await invoke('request_factory_reset')
  // Set the breadcrumb AFTER clearing local state so it survives to the toast.
  markRepairRequested('factory-reset')
  const { relaunch } = await import('@tauri-apps/plugin-process')
  await relaunch()
}
