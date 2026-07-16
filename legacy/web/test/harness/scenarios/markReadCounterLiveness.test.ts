/**
 * Scenario — counter-liveness regression guard, rewritten for the invalidation
 * model (RFC-L2-count-unification).
 *
 * The countDelta subsystem produced the same bug three times (§0 source-lag,
 * projection-less-drop, split-drop): every emit path had to attach deltas and
 * the client had to apply each exactly-once — a single miss froze the counter
 * until reload. It is DELETED. Counts now ride react-query invalidation: a
 * count-affecting `message.updated` invalidates the affected mailbox count key
 * and react-query refetches the runtime's canonical (trigger-maintained)
 * count. Self-correcting by construction — there is nothing to drop.
 *
 * This drives the REAL entity-store adapter + real wasm store over the fake
 * transport, and feeds delivered notification frames through the REAL
 * domain-cache event dispatch (`applyDomainEvent` — what `useDaemonEvents`
 * calls), with an ACTIVE react-query observer on `mailboxes('s')` whose
 * queryFn serves the scripted "server" (canonical) counts. Asserted behavior:
 * event → key invalidated → refetched count correct, for
 *
 *  (a) mark-read (unread--), sync-applied or echo — the same event shape;
 *  (b) a move between mailboxes (both sides refetch correct);
 *  (c) a trash/expunge (`deleted: true`, no projection);
 *  (d) the SPLIT topology path — the bare down-channel republish (no
 *      projection, broad change flags) still fires the invalidation (the
 *      third bug's class: that event used to be dropped whole);
 *  (e) a sync BURST coalesces into ~one refetch per window (leading +
 *      trailing), landing the final correct count;
 *  (f) the D2 overlay: the user's OWN mark-read adjusts the cached count
 *      immediately, then the echo's invalidation reconciles it to the server
 *      value.
 *
 * @spec docs/eph/RFC-L2-count-unification.md
 */
import { afterEach, describe, expect, it } from 'bun:test'
import { QueryObserver } from '@tanstack/react-query'
import type { QueryClient } from '@tanstack/react-query'

import { createClientHarness, messageUpdatedFrame } from '../index'
import { applyDomainEvent } from '../../../src/domainCache'
import { __resetCountInvalidationForTesting } from '../../../src/domain-cache/mailboxCounts'
import { __resetLiveStoreForTesting } from '../../../src/live-store/store'
import { queryKeys } from '../../../src/queryKeys'
import type { DomainEvent, Mailbox } from '../../../src/api/types'
import type {
  RuntimeFrame,
  RuntimeLinkViewRequest,
  RuntimeMailListViewState,
  RuntimeRunMutationRequest,
} from '../../../src/runtime/types'

const VIEW_REQUEST: RuntimeLinkViewRequest = {
  linkId: 'sess',
  view: {
    scope: { kind: 'source-mailbox', sourceId: 's', mailboxId: 'inbox' },
    limit: 50,
    sort: 'date',
    sortDir: 'desc',
    operation: { name: 'test' } as never,
  },
}

const mailbox = (
  id: string,
  role: string,
  unread: number,
  total: number,
): Mailbox =>
  ({
    id,
    name: id,
    role,
    unreadEmails: unread,
    totalEmails: total,
  }) as Mailbox

/** m1's post-mark-read projection (row liveness food; carries NO counts). */
const m1Read = {
  id: 'm1',
  sourceId: 's',
  receivedAt: '2026-04-29T10:00:00Z',
  keywords: ['$seen'],
  mailboxIds: ['inbox'],
  isRead: true,
  isFlagged: false,
  subject: 'm1',
}

function markRead(
  messageId: string,
  clientMutationId: string,
): RuntimeRunMutationRequest {
  return {
    linkId: 'sess',
    name: 'message.setKeywords',
    args: {
      sourceId: 's',
      messageId,
      command: { add: ['$seen'], remove: [] },
    },
    clientMutationId,
  }
}

/** The SPLIT topology's bare down-channel republish: an assertion WITHOUT a
 *  carried event synthesizes `{changes:{keywords,mailboxes}}` with no
 *  projection (`down_assertion_to_event`'s fallback). Under countDeltas this
 *  shape was dropped whole (the third bug); under invalidation it must still
 *  fire the count refetch. */
function bareSplitFrame(
  messageId: string,
  accountId = 's',
): RuntimeFrame<RuntimeMailListViewState> {
  return {
    type: 'notification',
    linkSeq: 102,
    kind: 'message.updated',
    payload: {
      seq: 3,
      accountId,
      topic: 'message.updated',
      occurredAt: 'now',
      payload: { messageId, changes: { keywords: true, mailboxes: true } },
    },
  } as RuntimeFrame<RuntimeMailListViewState>
}

/** A trash/expunge event: `deleted: true`, no `changes` object. */
function deletedFrame(
  messageId: string,
  accountId = 's',
): RuntimeFrame<RuntimeMailListViewState> {
  return {
    type: 'notification',
    linkSeq: 103,
    kind: 'message.updated',
    payload: {
      seq: 4,
      accountId,
      topic: 'message.updated',
      occurredAt: 'now',
      payload: { messageId, deleted: true },
    },
  } as RuntimeFrame<RuntimeMailListViewState>
}

/** Route delivered notification frames through the real domain-cache dispatch
 *  (what `useDaemonEvents` does in production). */
function dispatchNotifications(
  queryClient: QueryClient,
  frames: RuntimeFrame<RuntimeMailListViewState>[],
  from = 0,
): void {
  for (const frame of frames.slice(from)) {
    if (frame.type === 'notification') {
      applyDomainEvent(queryClient, frame.payload as DomainEvent)
    }
  }
}

/** Poll until `predicate` holds (invalidation refetches settle async). */
async function until(
  predicate: () => boolean,
  timeoutMs = 2000,
): Promise<void> {
  const deadline = Date.now() + timeoutMs
  while (!predicate()) {
    if (Date.now() > deadline) {
      throw new Error('condition not reached in time')
    }
    await new Promise((resolve) => setTimeout(resolve, 5))
  }
}

/** Mount an active observer over `mailboxes('s')` whose queryFn serves the
 *  scripted canonical counts, counting its fetches. */
function mountCountObserver(
  queryClient: QueryClient,
  server: () => Mailbox[],
): { fetches: () => number; unsubscribe: () => void } {
  let fetches = 0
  const observer = new QueryObserver<Mailbox[]>(queryClient, {
    queryKey: queryKeys.mailboxes('s'),
    queryFn: () => {
      fetches += 1
      return Promise.resolve(server())
    },
  })
  const unsubscribe = observer.subscribe(() => {})
  return { fetches: () => fetches, unsubscribe }
}

const inboxUnread = (queryClient: QueryClient): number | undefined =>
  queryClient
    .getQueryData<Mailbox[]>(queryKeys.mailboxes('s'))
    ?.find((m) => m.id === 'inbox')?.unreadEmails

let cleanup: (() => void)[] = []

afterEach(() => {
  // LIFO: observers unsubscribe before the harness (and its query client)
  // dispose.
  for (const fn of cleanup.reverse()) {
    fn()
  }
  cleanup = []
  __resetLiveStoreForTesting()
})

describe('scenario: mailbox counts refetch on invalidation (RFC-L2-count-unification)', () => {
  it('(a) a mark-read message.updated invalidates the count key and the refetch lands unread-1', async () => {
    const h = await createClientHarness({
      mailboxes: [mailbox('inbox', 'inbox', 1, 1)],
      rows: [
        { messageId: 'm1', receivedAt: '2026-04-29T10:00:00Z', keywords: [] },
      ],
    })
    cleanup.push(() => {
      __resetCountInvalidationForTesting(h.queryClient)
      h.dispose()
    })
    await h.openView(VIEW_REQUEST)

    // The canonical count the store's triggers maintain: after the mark-read
    // the server reads unread 0 / total 1.
    let server = [mailbox('inbox', 'inbox', 1, 1)]
    const observer = mountCountObserver(h.queryClient, () => server)
    cleanup.push(observer.unsubscribe)
    await until(() => observer.fetches() >= 1)

    server = [mailbox('inbox', 'inbox', 0, 1)]
    h.emitFrame(messageUpdatedFrame('m1', m1Read))
    await h.flush()
    dispatchNotifications(h.queryClient, h.frames)

    await until(() => inboxUnread(h.queryClient) === 0)
    expect(inboxUnread(h.queryClient)).toBe(0)
  })

  it('(b) a move invalidates once and the refetch lands BOTH sides correct', async () => {
    const h = await createClientHarness({
      mailboxes: [
        mailbox('inbox', 'inbox', 1, 1),
        mailbox('archive', 'archive', 0, 0),
      ],
      rows: [
        { messageId: 'm1', receivedAt: '2026-04-29T10:00:00Z', keywords: [] },
      ],
    })
    cleanup.push(() => {
      __resetCountInvalidationForTesting(h.queryClient)
      h.dispose()
    })
    await h.openView(VIEW_REQUEST)

    let server = [
      mailbox('inbox', 'inbox', 1, 1),
      mailbox('archive', 'archive', 0, 0),
    ]
    const observer = mountCountObserver(h.queryClient, () => server)
    cleanup.push(observer.unsubscribe)
    await until(() => observer.fetches() >= 1)

    // The move lands server-side: inbox empties, archive gains the unread.
    server = [
      mailbox('inbox', 'inbox', 0, 0),
      mailbox('archive', 'archive', 1, 1),
    ]
    h.emitFrame(
      messageUpdatedFrame(
        'm1',
        { ...m1Read, keywords: [], isRead: false, mailboxIds: ['archive'] },
        's',
        { mailboxes: true, arrived: true },
      ),
    )
    await h.flush()
    dispatchNotifications(h.queryClient, h.frames)

    await until(() => inboxUnread(h.queryClient) === 0)
    const rows = h.queryClient.getQueryData<Mailbox[]>(queryKeys.mailboxes('s'))
    expect(rows?.find((m) => m.id === 'inbox')?.totalEmails).toBe(0)
    expect(rows?.find((m) => m.id === 'archive')?.unreadEmails).toBe(1)
    expect(rows?.find((m) => m.id === 'archive')?.totalEmails).toBe(1)
  })

  it('(c) a trash/expunge (deleted:true, no changes object) still invalidates + refetches', async () => {
    const h = await createClientHarness({
      mailboxes: [mailbox('inbox', 'inbox', 1, 1)],
      rows: [
        { messageId: 'm1', receivedAt: '2026-04-29T10:00:00Z', keywords: [] },
      ],
    })
    cleanup.push(() => {
      __resetCountInvalidationForTesting(h.queryClient)
      h.dispose()
    })
    await h.openView(VIEW_REQUEST)

    let server = [mailbox('inbox', 'inbox', 1, 1)]
    const observer = mountCountObserver(h.queryClient, () => server)
    cleanup.push(observer.unsubscribe)
    await until(() => observer.fetches() >= 1)

    server = [mailbox('inbox', 'inbox', 0, 0)]
    h.emitFrame(deletedFrame('m1'))
    await h.flush()
    dispatchNotifications(h.queryClient, h.frames)

    await until(() => inboxUnread(h.queryClient) === 0)
    expect(
      h.queryClient
        .getQueryData<Mailbox[]>(queryKeys.mailboxes('s'))
        ?.find((m) => m.id === 'inbox')?.totalEmails,
    ).toBe(0)
  })

  it('(d) SPLIT: the bare down-channel republish (no projection) still fires the invalidation', async () => {
    // The third bug's class: a projection-less event over the link used to be
    // dropped whole, counts included. Under invalidation the event itself is
    // the trigger — no payload enrichment required.
    const h = await createClientHarness({
      mailboxes: [mailbox('inbox', 'inbox', 5, 9)],
      rows: [
        { messageId: 'm1', receivedAt: '2026-04-29T10:00:00Z', keywords: [] },
      ],
    })
    cleanup.push(() => {
      __resetCountInvalidationForTesting(h.queryClient)
      h.dispose()
    })
    await h.openView(VIEW_REQUEST)

    let server = [mailbox('inbox', 'inbox', 5, 9)]
    const observer = mountCountObserver(h.queryClient, () => server)
    cleanup.push(observer.unsubscribe)
    await until(() => observer.fetches() >= 1)

    // Another client marked a message read at the far node; the near node's
    // refetch (over the link) serves the updated canonical count.
    server = [mailbox('inbox', 'inbox', 4, 9)]
    h.emitFrame(bareSplitFrame('m1'))
    await h.flush()
    dispatchNotifications(h.queryClient, h.frames)

    await until(() => inboxUnread(h.queryClient) === 4)
    expect(inboxUnread(h.queryClient)).toBe(4)
  })

  it('(e) a burst of events coalesces into ~one refetch per window, final count correct', async () => {
    const h = await createClientHarness({
      mailboxes: [mailbox('inbox', 'inbox', 40, 40)],
      rows: [
        { messageId: 'm1', receivedAt: '2026-04-29T10:00:00Z', keywords: [] },
      ],
    })
    cleanup.push(() => {
      __resetCountInvalidationForTesting(h.queryClient)
      h.dispose()
    })
    await h.openView(VIEW_REQUEST)

    let unread = 40
    const observer = mountCountObserver(h.queryClient, () => [
      mailbox('inbox', 'inbox', unread, 40),
    ])
    cleanup.push(observer.unsubscribe)
    await until(() => observer.fetches() >= 1)
    const before = observer.fetches()

    // A sync burst: 20 count-affecting events land back-to-back while the
    // canonical count drains to 20.
    for (let i = 0; i < 20; i += 1) {
      unread -= 1
      h.emitFrame(
        messageUpdatedFrame(`m${i}`, {
          ...m1Read,
          id: `m${i}`,
          subject: `m${i}`,
        }),
      )
    }
    await h.flush()
    dispatchNotifications(h.queryClient, h.frames)

    // Leading fire lands immediately; the trailing fire (after the window)
    // reconciles to the final value. 20 events must NOT mean 20 refetches.
    await until(() => inboxUnread(h.queryClient) === 20, 3000)
    const burstFetches = observer.fetches() - before
    expect(burstFetches).toBeLessThanOrEqual(3)
    expect(inboxUnread(h.queryClient)).toBe(20)
  })

  it("(f) OVERLAY: the user's own mark-read adjusts the count immediately, then the echo reconciles — and the CANONICAL value wins", async () => {
    const h = await createClientHarness({
      mailboxes: [mailbox('inbox', 'inbox', 1, 1)],
      rows: [
        { messageId: 'm1', receivedAt: '2026-04-29T10:00:00Z', keywords: [] },
      ],
    })
    cleanup.push(() => {
      __resetCountInvalidationForTesting(h.queryClient)
      h.dispose()
    })
    await h.openView(VIEW_REQUEST)

    let server = [mailbox('inbox', 'inbox', 1, 1)]
    const observer = mountCountObserver(h.queryClient, () => server)
    cleanup.push(observer.unsubscribe)
    await until(() => observer.fetches() >= 1)

    // The user marks m1 read: the overlay decrements the cached count
    // IMMEDIATELY — before any event or refetch.
    await h.adapter.runRuntimeMutation(markRead('m1', 'c-mark-read'))
    await h.flush()
    expect(inboxUnread(h.queryClient)).toBe(0)

    // The settlement echo arrives. The canonical count DISAGREES with the
    // overlay's guess (two new unreads landed server-side meanwhile): only a
    // real invalidation + refetch can land 2. This closes the vacuous-pass
    // trap the old assertion had — checking the overlay's own value on a
    // never-invalidated query "passed" even when the echo was dropped whole
    // (reconciliation-correctness must be distinguishable from
    // overlay-correctness).
    server = [mailbox('inbox', 'inbox', 2, 3)]
    const fetchesBeforeEcho = observer.fetches()
    const frameCountBeforeEcho = h.frames.length
    h.emitFrame(messageUpdatedFrame('m1', m1Read))
    await h.flush()
    dispatchNotifications(h.queryClient, h.frames, frameCountBeforeEcho)

    await until(
      () =>
        observer.fetches() > fetchesBeforeEcho &&
        inboxUnread(h.queryClient) === 2,
    )
    expect(inboxUnread(h.queryClient)).toBe(2)
  })
})
