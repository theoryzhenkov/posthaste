import { afterEach, beforeEach, describe, expect, it } from 'bun:test'

import { QueryClient } from '@tanstack/react-query'

import type { DomainEvent } from '../src/api/types'
import { applyDomainEvent } from '../src/domainCache'
import { __resetCountInvalidationForTesting } from '../src/domain-cache/mailboxCounts'
import { mailKeys } from '../src/mailState'
import { queryKeys } from '../src/queryKeys'

// `message.updated` splits ownership (RFC-L2-count-unification): the entity
// store owns the mail-list ROWS (synthesized view frames — their REST
// invalidation stays retired), while mailbox COUNTS are react-query state — a
// count-affecting event INVALIDATES the count keys (`mailboxes(accountId)`,
// `smartMailboxes`) and react-query refetches the runtime's canonical counts.

function messageUpdated(changes: {
  arrived?: boolean
  mailboxes?: boolean
  keywords?: boolean
}): DomainEvent {
  return {
    seq: 1,
    accountId: 'primary',
    topic: 'message.updated',
    occurredAt: '2026-04-28T12:00:00Z',
    mailboxId: null,
    messageId: 'm1',
    payload: { changes },
  }
}

function seed(queryClient: QueryClient): void {
  // Seed the queries the handler invalidates so their state is observable.
  for (const key of [
    queryKeys.messagesRoot,
    queryKeys.conversationsRoot,
    queryKeys.mailboxes('primary'),
    queryKeys.smartMailboxes,
  ]) {
    queryClient.setQueryData(key, 'seeded')
  }
  // Seed the target message detail so `findConversationIdForMessage` returns
  // early (it otherwise iterates `messagesRoot` and expects `InfiniteData`).
  queryClient.setQueryData(mailKeys.message('primary', 'm1'), {
    conversationId: 'c1',
  } as never)
}

function invalidated(
  queryClient: QueryClient,
  key: readonly unknown[],
): boolean {
  return queryClient.getQueryState(key)?.isInvalidated ?? false
}

describe('applyDomainEvent message.updated (rows store-owned; counts invalidate)', () => {
  let queryClient: QueryClient

  beforeEach(() => {
    queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    })
  })

  afterEach(() => {
    __resetCountInvalidationForTesting(queryClient)
    queryClient.clear()
  })

  it('invalidates counts (not rows) on an arrival', () => {
    seed(queryClient)

    applyDomainEvent(queryClient, messageUpdated({ arrived: true }))

    // Store-owned ROWS: NOT invalidated (the store drives them via frames).
    expect(invalidated(queryClient, queryKeys.messagesRoot)).toBe(false)
    // COUNTS: invalidated → refetch the canonical counts.
    expect(invalidated(queryClient, queryKeys.mailboxes('primary'))).toBe(true)
    expect(invalidated(queryClient, queryKeys.smartMailboxes)).toBe(true)
    // Conversations remain non-store-owned: still invalidated.
    expect(invalidated(queryClient, queryKeys.conversationsRoot)).toBe(true)
  })

  it('invalidates counts on a keyword-only change (mark read/unread)', () => {
    seed(queryClient)

    applyDomainEvent(queryClient, messageUpdated({ keywords: true }))

    expect(invalidated(queryClient, queryKeys.messagesRoot)).toBe(false)
    expect(invalidated(queryClient, queryKeys.mailboxes('primary'))).toBe(true)
    expect(invalidated(queryClient, queryKeys.smartMailboxes)).toBe(true)
    expect(invalidated(queryClient, queryKeys.conversationsRoot)).toBe(true)
  })

  it('invalidates counts on a deletion (no changes object)', () => {
    seed(queryClient)

    const event = messageUpdated({})
    event.payload = { deleted: true }
    applyDomainEvent(queryClient, event)

    expect(invalidated(queryClient, queryKeys.messagesRoot)).toBe(false)
    expect(invalidated(queryClient, queryKeys.mailboxes('primary'))).toBe(true)
    expect(invalidated(queryClient, queryKeys.smartMailboxes)).toBe(true)
    expect(invalidated(queryClient, queryKeys.conversationsRoot)).toBe(true)
  })

  it('does NOT invalidate counts on a non-count-affecting update', () => {
    seed(queryClient)

    applyDomainEvent(queryClient, messageUpdated({}))

    expect(invalidated(queryClient, queryKeys.mailboxes('primary'))).toBe(false)
    expect(invalidated(queryClient, queryKeys.conversationsRoot)).toBe(true)
  })
})
