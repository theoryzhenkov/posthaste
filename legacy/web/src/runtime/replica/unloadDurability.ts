/**
 * Page-unload durability (W3 / N18): on `visibilitychange` -> hidden and
 * `pagehide`, flush any store op the active entity-store adapter has
 * queued/running, so a tab close can't strand an in-flight durable write
 * (the pending-set `put`/`remove` behind an optimistic mutation or an
 * incoming frame's settle).
 *
 * `visibilitychange` -> hidden fires while the page is still fully alive and
 * running JS — the reliable moment to await async work (covers the common
 * case: backgrounding a mobile tab, switching apps, or the tab-close
 * sequence, which hides before it unloads). `pagehide` is the unload-adjacent
 * backstop (covers back/forward-cache eviction). `beforeunload` is
 * deliberately NOT used alone: it is unreliable on mobile Safari/Chrome (may
 * not fire at all on tab close or app-switch) and its mere presence disables
 * the back-forward cache for the page.
 *
 * @spec docs/eph/RFC-L2-lifecycle-and-errors (W3 / N18)
 */
import { LOG_EVENTS, syncLogger } from '../../logger'

import { flushActiveEntityStore } from './entityStoreAdapter'

function flushOnHide(reason: 'visibilitychange' | 'pagehide'): void {
  // `flushActiveEntityStore` doesn't reject in practice (the controller's
  // `storeQueue` swallows failures into its tail) — the catch is a defensive
  // backstop so a future change there can't turn this into an unhandled
  // rejection at the least convenient moment (page teardown).
  flushActiveEntityStore().catch((error: unknown) => {
    syncLogger.warn(
      {
        event: LOG_EVENTS.replicaUnloadFlushFailed,
        reason,
        error: error instanceof Error ? error.message : String(error),
      },
      'entity-store unload flush failed',
    )
  })
}

function onVisibilityChange(): void {
  if (document.visibilityState === 'hidden') {
    flushOnHide('visibilitychange')
  }
}

function onPageHide(): void {
  flushOnHide('pagehide')
}

let installedOn: { document: Document; window: Window } | undefined

/**
 * Install the visibilitychange/pagehide durability hooks once. No-op outside
 * a DOM environment (SSR / a non-DOM test context that never registered
 * `document`).
 */
export function installUnloadDurabilityHooks(): void {
  if (installedOn || typeof document === 'undefined') {
    return
  }
  document.addEventListener('visibilitychange', onVisibilityChange)
  if (typeof window !== 'undefined') {
    window.addEventListener('pagehide', onPageHide)
    installedOn = { document, window }
  }
}

/** Test-only: undo `installUnloadDurabilityHooks` (removes the listeners it
 *  registered, not just the installed-once guard) so a test can re-install
 *  cleanly against the same DOM instance instead of accumulating listeners. */
export function resetUnloadDurabilityHooksForTesting(): void {
  if (!installedOn) {
    return
  }
  installedOn.document.removeEventListener(
    'visibilitychange',
    onVisibilityChange,
  )
  installedOn.window.removeEventListener('pagehide', onPageHide)
  installedOn = undefined
}
