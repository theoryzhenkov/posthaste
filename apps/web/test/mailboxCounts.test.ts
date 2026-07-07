/**
 * Unit tests for the count-unification module (RFC-L2-count-unification):
 * the pure overlay adjustment, the setQueryData cache adjust, and the
 * debounced (throttled) count invalidation.
 */
import { afterEach, describe, expect, it } from 'bun:test'
import { QueryClient, QueryObserver } from '@tanstack/react-query'

import type { Mailbox } from '../src/api/types'
import {
  __resetCountInvalidationForTesting,
  adjustMailboxCountsInCache,
  invalidateAllMailboxCounts,
  invalidateMailboxCountsDebounced,
} from '../src/domain-cache/mailboxCounts'
import { mailboxCountAdjustments } from '../src/runtime/replica/countOverlay'
import { queryKeys } from '../src/queryKeys'

const mailbox = (id: string, unread: number, total: number): Mailbox =>
  ({
    id,
    name: id,
    role: null,
    unreadEmails: unread,
    totalEmails: total,
  }) as unknown as Mailbox

const byId = (adjustments: ReturnType<typeof mailboxCountAdjustments>) =>
  Object.fromEntries(adjustments.map((a) => [a.mailboxId, a]))

describe('mailboxCountAdjustments (pure overlay math)', () => {
  it('mark-read decrements unread on every holding mailbox, totals untouched', () => {
    const adjustments = mailboxCountAdjustments(
      { mailboxIds: ['inbox', 'work'], isRead: false },
      { kind: 'setKeywords', add: ['$seen'], remove: [] },
    )
    expect(byId(adjustments)).toEqual({
      inbox: { mailboxId: 'inbox', unreadDelta: -1, totalDelta: 0 },
      work: { mailboxId: 'work', unreadDelta: -1, totalDelta: 0 },
    })
  })

  it('mark-unread increments unread', () => {
    const adjustments = mailboxCountAdjustments(
      { mailboxIds: ['inbox'], isRead: true },
      { kind: 'setKeywords', add: [], remove: ['$seen'] },
    )
    expect(byId(adjustments)).toEqual({
      inbox: { mailboxId: 'inbox', unreadDelta: 1, totalDelta: 0 },
    })
  })

  it('a no-op mark-read (already read) adjusts nothing', () => {
    expect(
      mailboxCountAdjustments(
        { mailboxIds: ['inbox'], isRead: true },
        { kind: 'setKeywords', add: ['$seen'], remove: [] },
      ),
    ).toEqual([])
  })

  it('a non-$seen keyword change adjusts nothing', () => {
    expect(
      mailboxCountAdjustments(
        { mailboxIds: ['inbox'], isRead: false },
        { kind: 'setKeywords', add: ['$flagged'], remove: [] },
      ),
    ).toEqual([])
  })

  it('a move adjusts BOTH sides, carrying the unread state', () => {
    const adjustments = mailboxCountAdjustments(
      { mailboxIds: ['inbox'], isRead: false },
      { kind: 'replaceMailboxes', mailboxIds: ['archive'] },
    )
    expect(byId(adjustments)).toEqual({
      inbox: { mailboxId: 'inbox', unreadDelta: -1, totalDelta: -1 },
      archive: { mailboxId: 'archive', unreadDelta: 1, totalDelta: 1 },
    })
  })

  it('a move of a READ message adjusts totals only', () => {
    const adjustments = mailboxCountAdjustments(
      { mailboxIds: ['inbox'], isRead: true },
      { kind: 'replaceMailboxes', mailboxIds: ['archive'] },
    )
    expect(byId(adjustments)).toEqual({
      inbox: { mailboxId: 'inbox', unreadDelta: 0, totalDelta: -1 },
      archive: { mailboxId: 'archive', unreadDelta: 0, totalDelta: 1 },
    })
  })

  it('destroy decrements totals (and unread if unread) on every holding mailbox', () => {
    const adjustments = mailboxCountAdjustments(
      { mailboxIds: ['trash'], isRead: false },
      { kind: 'destroy' },
    )
    expect(byId(adjustments)).toEqual({
      trash: { mailboxId: 'trash', unreadDelta: -1, totalDelta: -1 },
    })
  })

  it('applyDiff (undo vehicle) composes membership + read-state deltas', () => {
    // Undo of an archive+mark-read: back to inbox, back to unread.
    const adjustments = mailboxCountAdjustments(
      { mailboxIds: ['archive'], isRead: true },
      {
        kind: 'applyDiff',
        diff: {
          keywords: { added: [], removed: ['$seen'] },
          mailboxes: { added: ['inbox'], removed: ['archive'] },
        },
      },
    )
    expect(byId(adjustments)).toEqual({
      archive: { mailboxId: 'archive', unreadDelta: 0, totalDelta: -1 },
      inbox: { mailboxId: 'inbox', unreadDelta: 1, totalDelta: 1 },
    })
  })
})

describe('adjustMailboxCountsInCache', () => {
  it('adjusts the cached rows and clamps at zero', () => {
    const queryClient = new QueryClient()
    queryClient.setQueryData<Mailbox[]>(queryKeys.mailboxes('s'), [
      mailbox('inbox', 1, 3),
      mailbox('archive', 0, 0),
    ])
    adjustMailboxCountsInCache(queryClient, 's', [
      { mailboxId: 'inbox', unreadDelta: -2, totalDelta: -1 },
      { mailboxId: 'archive', unreadDelta: 1, totalDelta: 1 },
    ])
    const rows = queryClient.getQueryData<Mailbox[]>(queryKeys.mailboxes('s'))
    expect(rows?.find((m) => m.id === 'inbox')).toMatchObject({
      unreadEmails: 0, // clamped (1 - 2)
      totalEmails: 2,
    })
    expect(rows?.find((m) => m.id === 'archive')).toMatchObject({
      unreadEmails: 1,
      totalEmails: 1,
    })
    queryClient.clear()
  })

  it('is a no-op for an uncached account', () => {
    const queryClient = new QueryClient()
    adjustMailboxCountsInCache(queryClient, 'missing', [
      { mailboxId: 'inbox', unreadDelta: -1, totalDelta: 0 },
    ])
    expect(
      queryClient.getQueryData<Mailbox[]>(queryKeys.mailboxes('missing')),
    ).toBeUndefined()
    queryClient.clear()
  })
})

describe('invalidateMailboxCountsDebounced', () => {
  const clients: QueryClient[] = []
  const makeClient = () => {
    const queryClient = new QueryClient()
    clients.push(queryClient)
    return queryClient
  }

  afterEach(() => {
    for (const queryClient of clients.splice(0)) {
      __resetCountInvalidationForTesting(queryClient)
      queryClient.clear()
    }
  })

  const mountObserver = (queryClient: QueryClient, accountId: string) => {
    let fetches = 0
    let unread = 10
    const observer = new QueryObserver<Mailbox[]>(queryClient, {
      queryKey: queryKeys.mailboxes(accountId),
      queryFn: () => {
        fetches += 1
        unread -= 1
        return Promise.resolve([mailbox('inbox', unread, 10)])
      },
    })
    const unsubscribe = observer.subscribe(() => {})
    return { fetches: () => fetches, unsubscribe }
  }

  const settle = () => new Promise((resolve) => setTimeout(resolve, 10))

  it('a lone signal fires the refetch immediately (leading edge)', async () => {
    const queryClient = makeClient()
    const probe = mountObserver(queryClient, 's')
    await settle()
    const before = probe.fetches()

    invalidateMailboxCountsDebounced(queryClient, 's', 50)
    await settle()

    expect(probe.fetches()).toBe(before + 1)
    probe.unsubscribe()
  })

  it('a burst coalesces into one leading + one trailing fire, then closes the window', async () => {
    const queryClient = makeClient()
    const probe = mountObserver(queryClient, 's')
    await settle()
    const before = probe.fetches()

    for (let i = 0; i < 25; i += 1) {
      invalidateMailboxCountsDebounced(queryClient, 's', 50)
    }
    await settle()
    // Leading fire only so far.
    expect(probe.fetches()).toBe(before + 1)

    // After the window the ONE trailing fire reconciles the burst.
    await new Promise((resolve) => setTimeout(resolve, 70))
    expect(probe.fetches()).toBe(before + 2)

    // The window closed cleanly: a later lone signal fires immediately again.
    await new Promise((resolve) => setTimeout(resolve, 70))
    invalidateMailboxCountsDebounced(queryClient, 's', 50)
    await settle()
    expect(probe.fetches()).toBe(before + 3)
    probe.unsubscribe()
  })

  it("throttles per account: one account's burst does not defer another's fire", async () => {
    const queryClient = makeClient()
    const probeA = mountObserver(queryClient, 'a')
    const probeB = mountObserver(queryClient, 'b')
    await settle()
    const beforeA = probeA.fetches()
    const beforeB = probeB.fetches()

    invalidateMailboxCountsDebounced(queryClient, 'a', 50)
    invalidateMailboxCountsDebounced(queryClient, 'a', 50)
    invalidateMailboxCountsDebounced(queryClient, 'b', 50)
    await settle()

    expect(probeA.fetches()).toBe(beforeA + 1)
    expect(probeB.fetches()).toBe(beforeB + 1)
    probeA.unsubscribe()
    probeB.unsubscribe()
  })

  it("invalidateAllMailboxCounts marks every account's count query stale", () => {
    const queryClient = makeClient()
    queryClient.setQueryData<Mailbox[]>(queryKeys.mailboxes('a'), [
      mailbox('inbox', 1, 1),
    ])
    queryClient.setQueryData<Mailbox[]>(queryKeys.mailboxes('b'), [
      mailbox('inbox', 2, 2),
    ])
    invalidateAllMailboxCounts(queryClient)
    expect(
      queryClient.getQueryState(queryKeys.mailboxes('a'))?.isInvalidated,
    ).toBe(true)
    expect(
      queryClient.getQueryState(queryKeys.mailboxes('b'))?.isInvalidated,
    ).toBe(true)
  })
})
