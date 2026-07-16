/**
 * One-shot breadcrumbs so the client can confirm a completed local repair /
 * factory reset after the relaunch — these would otherwise give the user zero
 * feedback ("nothing happened").
 *
 * The breadcrumb lives in `localStorage`, which survives the IndexedDB replica
 * reset and the relaunch (a factory reset clears localStorage first, then sets
 * the breadcrumb, so it still survives). Read once on the next boot.
 */
export type RepairKind = 'repair' | 'factory-reset'

const REPAIR_BREADCRUMB_KEY = 'posthaste.repair.completed'
/** Ignore a stale breadcrumb (e.g. a relaunch that never completed). */
const FRESHNESS_MS = 5 * 60 * 1000

export function markRepairRequested(kind: RepairKind = 'repair'): void {
  try {
    window.localStorage.setItem(REPAIR_BREADCRUMB_KEY, `${kind}:${Date.now()}`)
  } catch {
    // Best-effort feedback; never block the repair on storage.
  }
}

/** The repair kind at most once after a recent repair; clears the breadcrumb. */
export function consumeRepairCompletion(): RepairKind | null {
  let raw: string | null = null
  try {
    raw = window.localStorage.getItem(REPAIR_BREADCRUMB_KEY)
    if (raw !== null) {
      window.localStorage.removeItem(REPAIR_BREADCRUMB_KEY)
    }
  } catch {
    return null
  }
  if (raw === null) {
    return null
  }
  const separator = raw.indexOf(':')
  const at = Number(raw.slice(separator + 1))
  if (!Number.isFinite(at) || Date.now() - at >= FRESHNESS_MS) {
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
  try {
    window.localStorage.clear()
  } catch {
    // Best-effort.
  }
}
