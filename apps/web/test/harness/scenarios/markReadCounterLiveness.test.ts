/**
 * Scenario — §0 client-liveness regression guard: the SOURCE mailbox unread
 * counter must move LIVE (no reload) when the user marks a message read.
 *
 * The 0.4.0 nightly has the §0 server-side fix (ff6b2d0d): the client-mutation
 * ECHO `message.updated` now carries projection + absolute `countDeltas`, the
 * SAME shape the sync-apply path emits. The owner still reproduces: marking a
 * message read flips the row's read-state but the sidebar mailbox unread COUNTER
 * does not decrement — neither on the optimistic echo NOR on a manual sync; only
 * a full reload fixes it.
 *
 * This drives the REAL entity-store adapter + real wasm store over the fake
 * transport and asserts the live-store counter slice (`useMailboxCounts` reads
 * `getMailboxCounts`) decrements from an enriched `message.updated`:
 *
 *  (a) SYNC path: an enriched `message.updated` arriving on the stream (a manual
 *      sync's re-emit) decrements the source counter, no reload.
 *  (b) ECHO path: a client `runMutation` mark-read followed by the enriched echo
 *      decrements the source counter, no reload.
 *
 * @spec docs/eph/AUDIT-L2-client-liveness.md (§0)
 */
import { afterEach, describe, expect, it } from 'bun:test'

import { createClientHarness, messageUpdatedFrame } from '../index'
import {
  __resetLiveStoreForTesting,
  getMailboxCounts,
} from '../../../src/live-store/store'
import type { Mailbox } from '../../../src/api/types'
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

const inbox = (unread: number, total: number): Mailbox =>
  ({
    id: 'inbox',
    name: 'Inbox',
    role: 'inbox',
    unreadEmails: unread,
    totalEmails: total,
  }) as Mailbox

/** m1 is unread in inbox; the seed row the view holds. */
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

/** A `message.updated` that carries `countDeltas` but NO projection — the shape a
 *  count-only metadata event (or the split-runtime down-channel) emits. The
 *  client must still apply its counts, not drop the whole event. */
function countOnlyFrame(
  messageId: string,
  countDeltas: Array<{
    mailboxId: string
    unreadCount: number
    totalCount: number
  }>,
  accountId = 's',
): RuntimeFrame<RuntimeMailListViewState> {
  return {
    type: 'notification',
    linkSeq: 101,
    kind: 'message.updated',
    payload: {
      seq: 2,
      accountId,
      topic: 'message.updated',
      occurredAt: 'now',
      payload: { messageId, countDeltas },
    },
  } as RuntimeFrame<RuntimeMailListViewState>
}

afterEach(() => {
  __resetLiveStoreForTesting()
})

describe('scenario §0: source-mailbox unread counter moves live on mark-read', () => {
  it('(a) SYNC: an enriched message.updated decrements the source counter, no reload', async () => {
    const h = await createClientHarness({
      mailboxes: [inbox(1, 1)],
      rows: [
        { messageId: 'm1', receivedAt: '2026-04-29T10:00:00Z', keywords: [] },
      ],
    })
    await h.openView(VIEW_REQUEST)

    // The manual sync re-emits an enriched message.updated: m1 now read, inbox
    // unread drops to 0 (absolute count).
    h.emitFrame(
      messageUpdatedFrame('m1', m1Read, [
        { mailboxId: 'inbox', unreadCount: 0, totalCount: 1 },
      ]),
    )
    await h.flush()

    expect(getMailboxCounts('s').inbox).toEqual({ unread: 0, total: 1 })

    h.dispose()
  })

  it('(b) ECHO: mark-read via runMutation + enriched echo decrements the source counter, no reload', async () => {
    const h = await createClientHarness({
      mailboxes: [inbox(1, 1)],
      rows: [
        { messageId: 'm1', receivedAt: '2026-04-29T10:00:00Z', keywords: [] },
      ],
    })
    await h.openView(VIEW_REQUEST)

    // The user marks m1 read — the optimistic fold flips the row (read-state),
    // but per §0 does NOT touch the count.
    await h.adapter.runRuntimeMutation(markRead('m1', 'c-mark-read'))
    await h.flush()

    // The client-mutation ECHO (the §0 fix's enriched event) arrives on the same
    // notification channel the sync uses: projection isRead:true + absolute
    // countDeltas unread 0.
    h.emitFrame(
      messageUpdatedFrame('m1', m1Read, [
        { mailboxId: 'inbox', unreadCount: 0, totalCount: 1 },
      ]),
    )
    await h.flush()

    // The sidebar counter (getMailboxCounts / useMailboxCounts) must decrement
    // LIVE — without a reload.
    expect(getMailboxCounts('s').inbox).toEqual({ unread: 0, total: 1 })

    h.dispose()
  })

  it('(c) COUNT-ONLY: a projection-less message.updated still applies its countDeltas (was dropped)', async () => {
    // Fail-before / pass-after: `storeUpdatesFromEvent` used to return null for a
    // non-deleted event with no projection, DROPPING its countDeltas — so the
    // counter slice ingested live counts from no such source and the sidebar
    // froze until reload. The fix routes the countDeltas as standalone
    // MailboxCount updates so the counter still moves.
    const h = await createClientHarness({
      mailboxes: [inbox(1, 1)],
      rows: [
        { messageId: 'm1', receivedAt: '2026-04-29T10:00:00Z', keywords: [] },
      ],
    })
    await h.openView(VIEW_REQUEST)

    h.emitFrame(
      countOnlyFrame('m1', [
        { mailboxId: 'inbox', unreadCount: 0, totalCount: 1 },
      ]),
    )
    await h.flush()

    expect(getMailboxCounts('s').inbox).toEqual({ unread: 0, total: 1 })

    h.dispose()
  })
})
