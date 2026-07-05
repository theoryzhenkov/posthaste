/**
 * Scenario — C1 / D113 (RC2): the COMPLETION of M44's reconcile pass for
 * COUNTS. The M44 recovery-edge reconcile re-serves view ROWS but never counts,
 * so after a missed count event or a reconnect the unread counters stayed frozen
 * until reload. This drives the whole client stack (real entity-store adapter +
 * real wasm store + the real `runtimeLinkClient`) over the fake transport and
 * proves:
 *
 *  (a) a source count that drifted during the disconnect is reconciled to the
 *      correct server value on the recovery edge, WITHOUT a reload — and the
 *      fresh server count REPLACES the stale seeded live count (shadow removed
 *      by writing the live-store OWNER);
 *  (b) the smart-mailbox counts query is refetched on the same edge;
 *  (c) steady-state A(1) is NOT regressed: a normal count delta updates the
 *      live count sub-second and the recovery reconcile does NOT fire on it.
 *
 * The count reconcile is wired to the SAME `onLinkReestablished` callback the
 * view re-open uses (`reconcileMailboxCountsOnRecovery`, registered by the
 * sidebar nav hook in production).
 *
 * @spec docs/eph/RFC-L2-client-resilience.md (M44, D112/D113)
 * @spec docs/eph/AUDIT-L2-client-liveness.md (C1)
 */
import { afterEach, describe, expect, it } from 'bun:test'
import { QueryClient, QueryObserver } from '@tanstack/react-query'

import { createClientHarness, messageUpdatedFrame } from '../index'
import { setRuntimeAdapterForTesting } from '../../../src/runtime/adapter'
import {
  resetRuntimeLinkClientForTesting,
  runtimeLinkClient,
} from '../../../src/runtime/linkClient'
import {
  __resetLiveStoreForTesting,
  getMailboxCounts,
  setMailboxCount,
} from '../../../src/live-store/store'
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

let restoreAdapter: (() => void) | undefined

afterEach(() => {
  restoreAdapter?.()
  restoreAdapter = undefined
  resetRuntimeLinkClientForTesting()
  __resetLiveStoreForTesting()
})

describe('scenario C1/D113: reconcile counts on the recovery edge', () => {
  it('reconciles a drifted source count to the fresh server value on the edge, no reload', async () => {
    const h = await createClientHarness()
    restoreAdapter = setRuntimeAdapterForTesting(h.adapter)
    resetRuntimeLinkClientForTesting()

    runtimeLinkClient.subscribe({ onFrame: () => {} })
    await runtimeLinkClient.openMessageListView(VIEW_REQUEST)

    // The sidebar nav hook registers the count reconcile on the recovery edge.
    runtimeLinkClient.onLinkReestablished(() => {
      void reconcileMailboxCountsOnRecovery(h.queryClient, ['s'])
    })

    // A count drifted during the disconnect: the live slice holds a stale 5,
    // while the server (refetched on recovery) now holds 0. Before the edge the
    // stale live count is what the sidebar reads.
    setMailboxCount('s', 'inbox', { unread: 5, total: 9 })
    h.queryClient.setQueryData<Mailbox[]>(queryKeys.mailboxes('s'), [
      mailbox(0, 8),
    ])
    expect(getMailboxCounts('s').inbox).toEqual({ unread: 5, total: 9 })

    // The engine re-prepared a FRESH link — the recovery edge fires. The count
    // reconcile is async (it awaits the count refetch); let it settle.
    h.transport.reestablishLink('link-B')
    await new Promise((resolve) => setTimeout(resolve, 0))
    await new Promise((resolve) => setTimeout(resolve, 0))

    // The fresh server count REPLACED the stale seeded live count in the owner
    // slice — the sidebar now reads 0 without a reload (shadow removed).
    expect(getMailboxCounts('s').inbox).toEqual({ unread: 0, total: 8 })

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

    qc.setQueryData<Mailbox[]>(queryKeys.mailboxes('s'), [mailbox(0, 8)])
    setMailboxCount('s', 'inbox', { unread: 5, total: 9 })

    await reconcileMailboxCountsOnRecovery(qc, ['s'])

    // Source count reseeded from fresh server data...
    expect(getMailboxCounts('s').inbox).toEqual({ unread: 0, total: 8 })
    // ...and the smart-mailbox counts query was refetched on the same edge.
    expect(smartFetches).toBeGreaterThan(initialFetches)

    unsubscribe()
    qc.clear()
  })

  it('does NOT reconcile on a normal count delta — A(1) steady state intact', async () => {
    const h = await createClientHarness()
    restoreAdapter = setRuntimeAdapterForTesting(h.adapter)
    resetRuntimeLinkClientForTesting()

    runtimeLinkClient.subscribe({ onFrame: () => {} })
    await runtimeLinkClient.openMessageListView(VIEW_REQUEST)

    // Wire the recovery reconcile, and script the server count to a value that
    // would be visible ONLY if the reconcile wrongly fired on the mutation.
    runtimeLinkClient.onLinkReestablished(() => {
      void reconcileMailboxCountsOnRecovery(h.queryClient, ['s'])
    })
    h.queryClient.setQueryData<Mailbox[]>(queryKeys.mailboxes('s'), [
      mailbox(99, 99),
    ])

    // A(1) steady-state path: a client-mutation echo carries an absolute count
    // delta → the live source count moves sub-second, with no reconcile.
    h.transport.emitFrame(
      messageUpdatedFrame(
        'm1',
        {
          id: 'm1',
          sourceId: 's',
          receivedAt: '2026-04-29T10:00:00Z',
          keywords: ['$seen'],
          mailboxIds: ['inbox'],
          isRead: true,
          isFlagged: false,
          subject: 'm1',
        },
        [{ mailboxId: 'inbox', unreadCount: 3, totalCount: 8 }],
      ),
    )
    await h.flush()

    // The live count reflects the delta (3), NOT the reconcile's server value
    // (99) — proving the recovery reconcile did not fire on a normal mutation.
    expect(getMailboxCounts('s').inbox).toEqual({ unread: 3, total: 8 })

    h.dispose()
  })
})
