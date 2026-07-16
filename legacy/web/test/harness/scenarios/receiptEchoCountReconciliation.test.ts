/**
 * Scenario — OWN-mutation count reconciliation rides the RECEIPT's bundled
 * echo, not only the link-stream echo (RFC-L2-count-unification, v0.5.0 field
 * regression).
 *
 * The v0.5.0-nightly.1 regression class: the user's own mark-read reconciled
 * counts ONLY through the link-stream `message.updated` echo. That delivery is
 * best-effort — the far end's lag/stale-cursor collapse re-serves view
 * snapshots + the live mutation window but never replays missed NOTIFICATION
 * frames (`crates/posthaste-runtime/src/far_end/links.rs`,
 * `collapse_link_frames`) — so a dropped echo left smart-mailbox counts frozen
 * while the D2 overlay made the SOURCE count merely LOOK right. The fix: the
 * settled receipt carries the command's events verbatim (`CommandAck.events`,
 * the BUNDLED ECHO) and the entity-store adapter dispatches them through
 * `applyDomainEvent` — the same domain-cache path the stream echo takes — so
 * the user's own mutation reconciles counts on the request/response channel,
 * which cannot be dropped by the stream.
 *
 * EVENT FIXTURES ARE PINNED TO THE REAL WIRE SHAPE — not hand-invented: the
 * `message.updated` echo below mirrors, field for field, the frame captured
 * verbatim from the real emitter by the Rust integration test
 * `own_set_keywords_echo_arrives_on_the_link_stream_with_change_flags`
 * (crates/posthaste-authority-server/tests/authority_server_handle.rs), whose
 * payload is built by `set_keywords_tx`
 * (crates/posthaste-store/src/mutations/commands.rs — `"changes": {
 * "keywords": true }` beside `keywords`/`assertion`/`projection`), enveloped
 * by the camelCase `DomainEvent` serde
 * (crates/posthaste-domain-model/src/model/records.rs). Only the ids/dates
 * are renamed to this harness's seeded fixture ('s'/'m1'/'inbox'); every key
 * and nesting level is the capture's. The same events ride the receipt as
 * `receipt.output.events` (pinned by the same Rust test).
 *
 * Covered here:
 *  (a) OWN mark-read, STREAM ECHO LOST: the receipt's bundled echo alone must
 *      invalidate + refetch the source AND smart counts — and the refetched
 *      CANONICAL value must win over the optimistic overlay's guess
 *      (reconciliation-correctness, not overlay-correctness: the scripted
 *      canonical intentionally DISAGREES with the overlay's decrement).
 *  (b) an EXTERNAL/sync `message.updated` (stream notification, same verbatim
 *      shape) → the same invalidation + refetch.
 *  (c) a non-count event (`message.body_cached`) → NO count refetch.
 *
 * @spec docs/eph/RFC-L2-count-unification.md
 */
import { afterEach, describe, expect, it } from 'bun:test'
import { QueryObserver } from '@tanstack/react-query'
import type { QueryClient } from '@tanstack/react-query'

import { createClientHarness } from '../index'
import { applyDomainEvent } from '../../../src/domainCache'
import { __resetCountInvalidationForTesting } from '../../../src/domain-cache/mailboxCounts'
import { __resetLiveStoreForTesting } from '../../../src/live-store/store'
import { queryKeys } from '../../../src/queryKeys'
import type {
  DomainEvent,
  Mailbox,
  SmartMailboxSummary,
} from '../../../src/api/types'
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

const mailbox = (id: string, unread: number, total: number): Mailbox =>
  ({
    id,
    name: id,
    role: id,
    unreadEmails: unread,
    totalEmails: total,
  }) as Mailbox

const smart = (id: string, unread: number): SmartMailboxSummary =>
  ({
    id,
    name: id,
    kind: 'query',
    defaultKey: null,
    role: null,
    parentId: null,
    unreadMessages: unread,
    totalMessages: unread,
    createdAt: 'now',
    updatedAt: 'now',
  }) as unknown as SmartMailboxSummary

/**
 * The post-mark-read `message.updated` DOMAIN EVENT, shape-verbatim from the
 * Rust capture (see the module doc): the envelope carries camelCase
 * seq/accountId/topic/occurredAt/mailboxId/messageId, and the payload carries
 * `changes.keywords` (the count gate) beside keywords/assertion/projection.
 */
function capturedMarkReadEcho(seq: number): DomainEvent {
  const projection = {
    conversationId: 'conv-m1',
    fromEmail: 'alice@example.com',
    fromName: 'Alice',
    hasAttachment: false,
    id: 'm1',
    isFlagged: false,
    isRead: true,
    keywords: ['$seen'],
    mailboxIds: ['inbox'],
    preview: 'Preview',
    receivedAt: '2026-04-29T10:00:00Z',
    rfcMessageId: '<m1@example.test>',
    sourceId: 's',
    sourceName: 's',
    sourceThreadId: 'thread-m1',
    subject: 'm1',
    to: [],
  }
  return {
    seq,
    accountId: 's',
    topic: 'message.updated',
    occurredAt: '2026-07-07T10:41:46.469639502Z',
    mailboxId: 'inbox',
    messageId: 'm1',
    payload: {
      assertion: { after: projection, before: null },
      changes: { keywords: true },
      keywords: ['$seen'],
      messageId: 'm1',
      projection,
    },
  }
}

function markRead(messageId: string): RuntimeRunMutationRequest {
  return {
    linkId: 'sess',
    name: 'message.setKeywords',
    args: {
      sourceId: 's',
      messageId,
      command: { add: ['$seen'], remove: [] },
    },
    clientMutationId: 'c-mark-read',
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

interface CountingObserver {
  fetches: () => number
  unsubscribe: () => void
}

function mountObserver<T>(
  queryClient: QueryClient,
  queryKey: readonly unknown[],
  server: () => T,
): CountingObserver {
  let fetches = 0
  const observer = new QueryObserver<T>(queryClient, {
    queryKey: queryKey as unknown[],
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

const smartUnread = (queryClient: QueryClient): number | undefined =>
  queryClient
    .getQueryData<SmartMailboxSummary[]>(queryKeys.smartMailboxes)
    ?.find((m) => m.id === 'smart-inbox')?.unreadMessages

let cleanup: (() => void)[] = []

afterEach(() => {
  for (const fn of cleanup.reverse()) {
    fn()
  }
  cleanup = []
  __resetLiveStoreForTesting()
})

describe('scenario: receipt bundled echo reconciles counts (RFC-L2-count-unification)', () => {
  it('(a) own mark-read with the STREAM ECHO LOST: the receipt echo invalidates + the canonical refetch WINS over the overlay', async () => {
    const h = await createClientHarness({
      mailboxes: [mailbox('inbox', 5, 9)],
      rows: [
        { messageId: 'm1', receivedAt: '2026-04-29T10:00:00Z', keywords: [] },
      ],
      // The settled receipt carries the command's events — the BUNDLED ECHO
      // (CommandAck { detail, events }, receipt.output on the wire).
      mutationOutput: () => ({
        detail: null,
        events: [capturedMarkReadEcho(23)],
      }),
    })
    cleanup.push(() => {
      __resetCountInvalidationForTesting(h.queryClient)
      h.dispose()
    })
    await h.openView(VIEW_REQUEST)

    // Canonical counts the "server" serves. After the mark-read the canonical
    // reads unread 3 — deliberately DIFFERENT from the overlay's local guess
    // (5 - 1 = 4): other unreads changed server-side meanwhile. Only a real
    // refetch can land 3, so this asserts reconciliation-correctness, not
    // overlay-correctness (the vacuous-pass trap the old (f) scenario had).
    let sourceServer = [mailbox('inbox', 5, 9)]
    let smartServer = [smart('smart-inbox', 5)]
    const sourceObserver = mountObserver(
      h.queryClient,
      queryKeys.mailboxes('s'),
      () => sourceServer,
    )
    const smartObserver = mountObserver(
      h.queryClient,
      queryKeys.smartMailboxes,
      () => smartServer,
    )
    cleanup.push(sourceObserver.unsubscribe, smartObserver.unsubscribe)
    await until(
      () => sourceObserver.fetches() >= 1 && smartObserver.fetches() >= 1,
    )
    const sourceFetches = sourceObserver.fetches()
    const smartFetches = smartObserver.fetches()

    sourceServer = [mailbox('inbox', 3, 9)]
    smartServer = [smart('smart-inbox', 3)]

    // The user's OWN mark-read. The fake transport (a dead-stream stand-in)
    // never emits the notification frame — the receipt is the only echo.
    await h.adapter.runRuntimeMutation(markRead('m1'))
    await h.flush()

    await until(
      () =>
        sourceObserver.fetches() > sourceFetches &&
        smartObserver.fetches() > smartFetches &&
        inboxUnread(h.queryClient) === 3 &&
        smartUnread(h.queryClient) === 3,
    )
    expect(inboxUnread(h.queryClient)).toBe(3)
    expect(smartUnread(h.queryClient)).toBe(3)
  })

  it('(b) an EXTERNAL message.updated (stream notification, verbatim shape) refetches the smart count', async () => {
    const h = await createClientHarness({
      mailboxes: [mailbox('inbox', 5, 9)],
      rows: [
        { messageId: 'm1', receivedAt: '2026-04-29T10:00:00Z', keywords: [] },
      ],
    })
    cleanup.push(() => {
      __resetCountInvalidationForTesting(h.queryClient)
      h.dispose()
    })
    await h.openView(VIEW_REQUEST)

    let smartServer = [smart('smart-inbox', 5)]
    const smartObserver = mountObserver(
      h.queryClient,
      queryKeys.smartMailboxes,
      () => smartServer,
    )
    cleanup.push(smartObserver.unsubscribe)
    await until(() => smartObserver.fetches() >= 1)

    smartServer = [smart('smart-inbox', 4)]
    // Another client marked m1 read: the runtime broadcasts the SAME
    // wire-shaped event as a notification frame.
    const frame = {
      type: 'notification',
      linkSeq: 101,
      kind: 'message.updated',
      payload: capturedMarkReadEcho(24),
    } as RuntimeFrame<RuntimeMailListViewState>
    h.emitFrame(frame)
    await h.flush()
    // Route delivered notification frames through the real domain-cache
    // dispatch (what `useDaemonEvents` does in production).
    for (const delivered of h.frames) {
      if (delivered.type === 'notification') {
        applyDomainEvent(h.queryClient, delivered.payload as DomainEvent)
      }
    }

    await until(() => smartUnread(h.queryClient) === 4)
    expect(smartUnread(h.queryClient)).toBe(4)
  })

  it('(c) a non-count event (message.body_cached) fires NO count refetch', async () => {
    const h = await createClientHarness({
      mailboxes: [mailbox('inbox', 5, 9)],
      rows: [
        { messageId: 'm1', receivedAt: '2026-04-29T10:00:00Z', keywords: [] },
      ],
    })
    cleanup.push(() => {
      __resetCountInvalidationForTesting(h.queryClient)
      h.dispose()
    })
    await h.openView(VIEW_REQUEST)

    const smartObserver = mountObserver(
      h.queryClient,
      queryKeys.smartMailboxes,
      () => [smart('smart-inbox', 5)],
    )
    const sourceObserver = mountObserver(
      h.queryClient,
      queryKeys.mailboxes('s'),
      () => [mailbox('inbox', 5, 9)],
    )
    cleanup.push(smartObserver.unsubscribe, sourceObserver.unsubscribe)
    await until(
      () => smartObserver.fetches() >= 1 && sourceObserver.fetches() >= 1,
    )
    const smartFetches = smartObserver.fetches()
    const sourceFetches = sourceObserver.fetches()

    // The real body-cached emission (`apply_message_body_tx`,
    // crates/posthaste-store/src/mutations/commands.rs): payload is just the
    // messageId — no changes flags, no deletion.
    applyDomainEvent(h.queryClient, {
      seq: 30,
      accountId: 's',
      topic: 'message.body_cached',
      occurredAt: '2026-07-07T10:41:46.469639502Z',
      mailboxId: 'inbox',
      messageId: 'm1',
      payload: { messageId: 'm1' },
    })
    await new Promise((resolve) => setTimeout(resolve, 50))

    expect(smartObserver.fetches()).toBe(smartFetches)
    expect(sourceObserver.fetches()).toBe(sourceFetches)
  })
})
