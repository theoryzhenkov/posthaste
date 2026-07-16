/**
 * The one TS reactive store — the dumb main-thread mirror (D115).
 *
 * The wasm replica (in the worker) stays the SOVEREIGN source of domain truth
 * and ALL computation: folds and view projection. This module holds NO logic —
 * it is the latest projected state plus a subscription notify, and it exists
 * only because:
 *
 *  - React's `useSyncExternalStore` needs a synchronous `getSnapshot`, but the
 *    replica is a `postMessage` away; and
 *  - infra state (connection health) is not domain state and cannot live in the
 *    replica.
 *
 * Two slices:
 *
 *  (a) view projections, keyed by `viewKey` — the projected rows the adapter
 *      re-derives from the replica;
 *  (b) connection health — a placeholder the D112/M44 health FSM will drive.
 *
 * Mailbox COUNTS no longer live here (RFC-L2-count-unification): counts are
 * react-query state — the `mailboxes(accountId)` / `smartMailboxes` queries
 * carry the runtime's canonical counts, invalidated on count-affecting events
 * and adjusted optimistically for the user's own mutations
 * (`domain-cache/mailboxCounts.ts`).
 *
 * PURITY CONTRACT: this module imports NOTHING from react-query or the
 * entity-store adapter. Producers (the adapter) import the store; the store
 * never imports a producer. The only imports are types. Enforced by the M46
 * grep gate.
 *
 * @spec docs/eph/RFC-L2-client-resilience.md#D115
 */
import { useSyncExternalStore } from 'react'

import type { RuntimeMailListRowState } from '@/runtime/types'

/** The connection-health FSM state (D112). `healthy` until M44 drives it. */
export type ConnectionHealth = 'healthy' | 'degraded' | 'recovering'

// --- Stable empty references, so an absent slice never mints a new snapshot. ---
const EMPTY_ROWS: readonly RuntimeMailListRowState[] = Object.freeze([])

// --- Slice state (module-level; the store is a process-wide singleton). ---
let viewProjections: Record<string, readonly RuntimeMailListRowState[]> = {}
let connectionHealth: ConnectionHealth = 'healthy'

const listeners = new Set<() => void>()

function emit(): void {
  for (const listener of listeners) {
    listener()
  }
}

/** External-store subscribe: register a listener, return its unsubscribe. */
function subscribe(listener: () => void): () => void {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

// --- (a) View projections -----------------------------------------------------

/**
 * Mirror the latest projected rows for a view. A stable reference is kept until
 * the rows actually change so `useLiveView` never re-renders on a no-op write.
 */
export function setViewProjection(
  viewKey: string,
  rows: readonly RuntimeMailListRowState[],
): void {
  if (viewProjections[viewKey] === rows) {
    return
  }
  viewProjections = { ...viewProjections, [viewKey]: rows }
  emit()
}

/** Drop a view's projection (on view close). */
export function clearViewProjection(viewKey: string): void {
  if (!(viewKey in viewProjections)) {
    return
  }
  const next = { ...viewProjections }
  delete next[viewKey]
  viewProjections = next
  emit()
}

/** The latest mirrored rows for a view, or a stable empty array when absent. */
export function getViewProjection(
  viewKey: string,
): readonly RuntimeMailListRowState[] {
  return viewProjections[viewKey] ?? EMPTY_ROWS
}

// --- (b) Connection health ----------------------------------------------------

/** Set the connection-health state (the D112/M44 FSM's writer). */
export function setConnectionHealth(next: ConnectionHealth): void {
  if (connectionHealth === next) {
    return
  }
  connectionHealth = next
  emit()
}

/** The current connection-health state. */
export function getConnectionHealth(): ConnectionHealth {
  return connectionHealth
}

// --- Hooks (thin useSyncExternalStore wrappers with stable snapshots) ---------

/** Subscribe to a view's mirrored projected rows. */
export function useLiveView(
  viewKey: string,
): readonly RuntimeMailListRowState[] {
  const snapshot = (): readonly RuntimeMailListRowState[] =>
    getViewProjection(viewKey)
  return useSyncExternalStore(subscribe, snapshot, snapshot)
}

/** Subscribe to the connection-health state (D112/M44). */
export function useConnectionHealth(): ConnectionHealth {
  return useSyncExternalStore(
    subscribe,
    getConnectionHealth,
    getConnectionHealth,
  )
}

/**
 * Test-only: reset every slice + drop all listeners. The store is a module
 * singleton, so tests that drive producers must reset between cases.
 */
export function __resetLiveStoreForTesting(): void {
  viewProjections = {}
  connectionHealth = 'healthy'
  listeners.clear()
}
