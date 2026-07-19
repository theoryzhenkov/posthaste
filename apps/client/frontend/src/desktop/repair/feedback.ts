/**
 * One-shot breadcrumbs so the client can confirm a completed local repair /
 * factory reset after the relaunch — these would otherwise give the user zero
 * feedback ("nothing happened").
 *
 * The breadcrumb lives in `localStorage`, which survives the IndexedDB replica
 * reset and the relaunch (a factory reset clears localStorage first, then sets
 * the breadcrumb, so it still survives). Read once on the next boot.
 * Everything here is best-effort feedback: storage access rides the R8 seam
 * (lib/ambient/storage), which swallows blocked storage.
 */
import {
  clearAmbientStorage,
  readStorageItem,
  removeStorageItem,
  writeStorageItem,
} from '@/lib/ambient/storage'
import { nowMs } from '@/lib/ambient/time'

export type RepairKind = 'repair' | 'factory-reset'

const REPAIR_BREADCRUMB_KEY = 'posthaste.repair.completed'
/** Ignore a stale breadcrumb (e.g. a relaunch that never completed). */
const FRESHNESS_MS = 5 * 60 * 1000

export function markRepairRequested(kind: RepairKind = 'repair'): void {
  writeStorageItem(REPAIR_BREADCRUMB_KEY, `${kind}:${nowMs()}`)
}

/** The repair kind at most once after a recent repair; clears the breadcrumb. */
export function consumeRepairCompletion(): RepairKind | null {
  const raw = readStorageItem(REPAIR_BREADCRUMB_KEY)
  if (raw === null) {
    return null
  }
  removeStorageItem(REPAIR_BREADCRUMB_KEY)
  const separator = raw.indexOf(':')
  const at = Number(raw.slice(separator + 1))
  if (!Number.isFinite(at) || nowMs() - at >= FRESHNESS_MS) {
    return null
  }
  return raw.slice(0, separator) === 'factory-reset'
    ? 'factory-reset'
    : 'repair'
}

/**
 * Wipe all client-owned local UI state (preferences, layout, dev-tools, the
 * web connection store) for a factory reset. The IndexedDB replica is cleared
 * separately. Call BEFORE setting the breadcrumb so the breadcrumb survives.
 */
export function clearClientLocalState(): void {
  clearAmbientStorage()
}
