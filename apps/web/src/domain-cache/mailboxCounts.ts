/**
 * Mailbox COUNTS on react-query invalidation (RFC-L2-count-unification).
 *
 * The one count mechanism for source AND smart mailboxes: the authoritative
 * count is the runtime's canonical trigger-maintained `unreadEmails`/
 * `totalEmails` on the mailbox rows, read through the existing
 * `queryKeys.mailboxes(accountId)` / `queryKeys.smartMailboxes` queries. On any
 * count-affecting signal (a `message.updated` event — bundled echo, sync apply,
 * or the split runtime's down-channel republish — sync completion, mutation
 * settlement) the affected keys are INVALIDATED and react-query refetches the
 * current value. No delta computation, no exactly-once client application: a
 * missed invalidation is at worst a beat of staleness the next event corrects —
 * never a wrong-until-reload count (the countDelta subsystem's failure mode,
 * three times).
 *
 * Two pieces live here:
 *
 *  1. **Debounced invalidation** — a sync burst streams thousands of
 *     `message.updated` events; invalidating per event would refetch-stampede
 *     the count queries. {@link invalidateMailboxCountsDebounced} throttles per
 *     (queryClient, account): the first signal fires immediately (a lone
 *     mark-read stays sub-second live), further signals inside the window
 *     coalesce into ONE trailing invalidation whose refetch lands the final
 *     correct value.
 *
 *  2. **The thin optimistic overlay** (D2) — for the user's OWN mutation the
 *     refetch round-trip would read as lag, so the affected count entries are
 *     adjusted immediately via `setQueryData` and reconciled by the
 *     invalidation refetch the settlement echo triggers. Self-correcting: the
 *     overlay is an adjustment on the cached query rows, never a second count
 *     store — a missed overlay means the refetch lands a beat later, never a
 *     permanently-wrong count. (The assertion→adjustment math lives with the
 *     adapter — `runtime/replica/countOverlay.ts` — because it is shaped by
 *     the replica's assertion vocabulary; this module owns only the
 *     react-query cache surface.)
 */
import type { QueryClient } from '@tanstack/react-query'

import type { Mailbox } from '../api/types'
import { queryKeys } from '../queryKeys'

/** How long a count-invalidation window stays open (per account). One refetch
 * fires at the leading edge; a burst coalesces into one trailing refetch. */
export const COUNT_INVALIDATION_WINDOW_MS = 300

/** A per-mailbox count adjustment the optimistic overlay applies. */
export interface MailboxCountAdjustment {
  mailboxId: string
  unreadDelta: number
  totalDelta: number
}

/**
 * Apply overlay adjustments onto the cached `mailboxes(accountId)` rows
 * (`setQueryData`; clamped at 0). A no-op for an uncached account — the
 * eventual refetch then simply serves the fresh value.
 */
export function adjustMailboxCountsInCache(
  queryClient: QueryClient,
  accountId: string,
  adjustments: readonly MailboxCountAdjustment[],
): void {
  if (adjustments.length === 0) {
    return
  }
  const byMailbox = new Map(adjustments.map((a) => [a.mailboxId, a]))
  queryClient.setQueryData<Mailbox[]>(
    queryKeys.mailboxes(accountId),
    (current) =>
      current?.map((mailbox) => {
        const adjustment = byMailbox.get(mailbox.id)
        if (!adjustment) {
          return mailbox
        }
        return {
          ...mailbox,
          unreadEmails: Math.max(
            0,
            mailbox.unreadEmails + adjustment.unreadDelta,
          ),
          totalEmails: Math.max(0, mailbox.totalEmails + adjustment.totalDelta),
        }
      }),
  )
}

/** The count read models one invalidation fire refreshes: the account's source
 * mailbox rows (canonical counts), the smart-mailbox counts, and the
 * mail-navigation bootstrap + tags (whose hydrate re-seeds the same caches). */
function fireCountInvalidation(
  queryClient: QueryClient,
  accountId: string,
): void {
  void queryClient.invalidateQueries({
    queryKey: queryKeys.mailboxes(accountId),
  })
  void queryClient.invalidateQueries({ queryKey: queryKeys.smartMailboxes })
  void queryClient.invalidateQueries({ queryKey: queryKeys.mailNavigationRead })
  void queryClient.invalidateQueries({ queryKey: queryKeys.tags })
}

interface ThrottleWindow {
  timer: ReturnType<typeof setTimeout>
  trailing: boolean
}

// Per-queryClient throttle state (WeakMap: a disposed test QueryClient drops
// its windows with it). Keyed by account inside.
const throttleWindows = new WeakMap<QueryClient, Map<string, ThrottleWindow>>()

/**
 * Invalidate an account's count read models, coalescing bursts: the FIRST
 * signal in a window fires immediately (liveness — a lone mark-read refetches
 * sub-second); further signals within `windowMs` fold into one trailing fire
 * whose refetch lands the final correct counts. A busy sync therefore costs at
 * most ~one refetch per window per account instead of one per event.
 */
export function invalidateMailboxCountsDebounced(
  queryClient: QueryClient,
  accountId: string,
  windowMs: number = COUNT_INVALIDATION_WINDOW_MS,
): void {
  let byAccount = throttleWindows.get(queryClient)
  if (!byAccount) {
    byAccount = new Map()
    throttleWindows.set(queryClient, byAccount)
  }
  const open = byAccount.get(accountId)
  if (open) {
    open.trailing = true
    return
  }
  fireCountInvalidation(queryClient, accountId)
  const startWindow = (): ThrottleWindow => ({
    timer: setTimeout(() => {
      const window = byAccount.get(accountId)
      if (!window) {
        return
      }
      if (window.trailing) {
        fireCountInvalidation(queryClient, accountId)
        byAccount.set(accountId, startWindow())
      } else {
        byAccount.delete(accountId)
      }
    }, windowMs),
    trailing: false,
  })
  byAccount.set(accountId, startWindow())
}

/**
 * Reconcile EVERY account's counts now (no debounce): the failed-mutation
 * revert path, where only the client mutation id is known — a failed mutation
 * is rare, so the broad refetch is cheaper than threading per-account
 * bookkeeping through settlement. Also clears any optimistic overlay left by
 * the reverted mutation.
 */
export function invalidateAllMailboxCounts(queryClient: QueryClient): void {
  // The root prefix covers every `['mailboxes', accountId]` key.
  void queryClient.invalidateQueries({ queryKey: ['mailboxes'] })
  void queryClient.invalidateQueries({ queryKey: queryKeys.smartMailboxes })
  void queryClient.invalidateQueries({ queryKey: queryKeys.mailNavigationRead })
}

/** Test-only: drop a query client's open throttle windows (clears timers). */
export function __resetCountInvalidationForTesting(
  queryClient: QueryClient,
): void {
  const byAccount = throttleWindows.get(queryClient)
  if (!byAccount) {
    return
  }
  for (const window of byAccount.values()) {
    clearTimeout(window.timer)
  }
  throttleWindows.delete(queryClient)
}
