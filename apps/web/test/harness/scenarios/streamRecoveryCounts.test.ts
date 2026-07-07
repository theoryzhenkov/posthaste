/**
 * Scenario — C1 / D113 (RC2): the COMPLETION of M44's reconcile pass for
 * COUNTS, on the invalidation model (RFC-L2-count-unification). An
 * invalidation missed during a disconnect would leave a count stale until the
 * next count-affecting event; the recovery edge closes the gap by refetching
 * the authoritative count queries — the SAME react-query keys every count
 * consumer reads (there is no separate live-count owner anymore).
 *
 * Proven here, driving the whole client stack (real entity-store adapter +
 * real wasm store + the real `runtimeLinkClient`) over the fake transport:
 *
 *  (a) a source count that drifted during the disconnect is refetched to the
 *      correct server value on the recovery edge, WITHOUT a reload;
 *  (b) the smart-mailbox counts query is refetched on the same edge;
 *  (c) the reconcile does NOT fire outside the recovery edge (steady-state
 *      count updates ride the event-driven invalidation instead).
 *
 * The count reconcile is wired to the SAME `onLinkReestablished` callback the
 * view re-open uses (`reconcileMailboxCountsOnRecovery`, registered by the
 * sidebar nav hook in production).
 *
 * @spec docs/eph/RFC-L2-client-resilience.md (M44, D112/D113)
 * @spec docs/eph/RFC-L2-count-unification.md
 */
import { afterEach, describe, expect, it } from 'bun:test'
import { QueryClient, QueryObserver } from '@tanstack/react-query'

import { createClientHarness } from '../index'
import { setRuntimeAdapterForTesting } from '../../../src/runtime/adapter'
import {
  resetRuntimeLinkClientForTesting,
  runtimeLinkClient,
} from '../../../src/runtime/linkClient'
import { __resetLiveStoreForTesting } from '../../../src/live-store/store'
import { reconcileMailboxCountsOnRecovery } from '../../../src/mailboxNavigationReadModels'
import { queryKeys } from '../../../src/queryKeys'
import type { Mailbox } from '../../../src/api/types'
import type { RuntimeMessagePageRequest } from '../../../src/runtime/types'

const VIEW_REQUEST: RuntimeMessagePageRequest = {
  scope: { kind: 'source-mailbox', sourceId: 's', mailboxId: 'inbox' },
  limit: 50,
  sort: 'date',
  sortDir: 'desc',
  operation: { name: 'test' } as never,
}

const mailbox = (unreadEmails: number, totalEmails: number): Mailbox =>
  ({
    id: 'inbox',
    name: 'Inbox',
    role: 'inbox',
    unreadEmails,
    totalEmails,
  }) as Mailbox

const inboxUnread = (queryClient: QueryClient): number | undefined =>
  queryClient
    .getQueryData<Mailbox[]>(queryKeys.mailboxes('s'))
    ?.find((m) => m.id === 'inbox')?.unreadEmails

let restoreAdapter: (() => void) | undefined

afterEach(() => {
  restoreAdapter?.()
  restoreAdapter = undefined
  resetRuntimeLinkClientForTesting()
  __resetLiveStoreForTesting()
})

describe('scenario C1/D113: reconcile counts on the recovery edge', () => {
  it('refetches a drifted source count to the fresh server value on the edge, no reload', async () => {
    const h = await createClientHarness()
    restoreAdapter = setRuntimeAdapterForTesting(h.adapter)
    resetRuntimeLinkClientForTesting()

    runtimeLinkClient.subscribe({ onFrame: () => {} })
    await runtimeLinkClient.openMessageListView(VIEW_REQUEST)

    // The sidebar nav hook registers the count reconcile on the recovery edge.
    runtimeLinkClient.onLinkReestablished(() => {
      void reconcileMailboxCountsOnRecovery(h.queryClient, ['s'])
    })

    // A mounted sidebar observer of the count query, whose queryFn serves the
    // CURRENT server value. During the disconnect the cached count (5) drifted
    // from the server truth (0) — an invalidation was missed.
    let serverUnread = 5
    const observer = new QueryObserver<Mailbox[]>(h.queryClient, {
      queryKey: queryKeys.mailboxes('s'),
      queryFn: () => Promise.resolve([mailbox(serverUnread, 8)]),
    })
    const unsubscribe = observer.subscribe(() => {})
    await new Promise((resolve) => setTimeout(resolve, 0))
    expect(inboxUnread(h.queryClient)).toBe(5)
    serverUnread = 0

    // The engine re-prepared a FRESH link — the recovery edge fires. The count
    // reconcile is async (it awaits the count refetch); let it settle.
    h.transport.reestablishLink('link-B')
    await new Promise((resolve) => setTimeout(resolve, 0))
    await new Promise((resolve) => setTimeout(resolve, 0))

    // The refetch replaced the stale cached count — the sidebar (which reads
    // this query) now shows 0 without a reload.
    expect(inboxUnread(h.queryClient)).toBe(0)

    unsubscribe()
    h.dispose()
  })

  it('refetches the smart-mailbox counts query on the recovery edge', async () => {
    const qc = new QueryClient()
    let smartFetches = 0
    // A mounted smart-mailboxes observer (the sidebar's) whose queryFn counts
    // its fetches, so we can prove the recovery reconcile refetches it.
    const observer = new QueryObserver(qc, {
      queryKey: queryKeys.smartMailboxes,
      queryFn: async () => {
        smartFetches += 1
        return []
      },
    })
    const unsubscribe = observer.subscribe(() => {})
    // Let the initial fetch settle.
    await new Promise((resolve) => setTimeout(resolve, 0))
    const initialFetches = smartFetches

    await reconcileMailboxCountsOnRecovery(qc, ['s'])

    expect(smartFetches).toBeGreaterThan(initialFetches)

    unsubscribe()
    qc.clear()
  })

  it('does NOT reconcile outside the recovery edge — steady state rides event invalidation', async () => {
    const h = await createClientHarness()
    restoreAdapter = setRuntimeAdapterForTesting(h.adapter)
    resetRuntimeLinkClientForTesting()

    runtimeLinkClient.subscribe({ onFrame: () => {} })
    await runtimeLinkClient.openMessageListView(VIEW_REQUEST)

    // Wire the recovery reconcile; count refetches through the observer.
    runtimeLinkClient.onLinkReestablished(() => {
      void reconcileMailboxCountsOnRecovery(h.queryClient, ['s'])
    })
    let fetches = 0
    const observer = new QueryObserver<Mailbox[]>(h.queryClient, {
      queryKey: queryKeys.mailboxes('s'),
      queryFn: () => {
        fetches += 1
        return Promise.resolve([mailbox(99, 99)])
      },
    })
    const unsubscribe = observer.subscribe(() => {})
    await new Promise((resolve) => setTimeout(resolve, 0))
    const initialFetches = fetches

    // Ordinary time passes with no recovery edge: the reconcile must not fire
    // — no refetch beyond the mount fetch.
    await new Promise((resolve) => setTimeout(resolve, 10))
    expect(fetches).toBe(initialFetches)

    unsubscribe()
    h.dispose()
  })
})
