import { afterEach, beforeEach, describe, expect, it } from 'bun:test'

import { QueryClient } from '@tanstack/react-query'

import type { DomainEvent } from '../src/api/types'
import { applyDomainEvent } from '../src/domainCache'
import { mailKeys } from '../src/mailState'
import { queryKeys } from '../src/queryKeys'

// `message.updated` is the event the entity store owns: it drives the mail-list
// rows (synthesized view frames) + the mailbox counts (`setQueryData`). The
// redundant REST invalidations for those store-owned keys are retired
// unconditionally (the store has no REST fallback), while the surfaces the store
// does not own (conversations, smart-mailboxes) still invalidate.

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

describe('applyDomainEvent message.updated (store-owned invalidations retired)', () => {
  let queryClient: QueryClient

  beforeEach(() => {
    queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    })
  })

  afterEach(() => {
    queryClient.clear()
  })

  it('skips rows + counts (the store owns them)', () => {
    seed(queryClient)

    applyDomainEvent(queryClient, messageUpdated({ arrived: true }))

    // Store-owned: NOT invalidated (the store drives rows via view frames +
    // counts via setQueryData).
    expect(invalidated(queryClient, queryKeys.messagesRoot)).toBe(false)
    expect(invalidated(queryClient, queryKeys.mailboxes('primary'))).toBe(false)
    // Not store-owned: still invalidated.
    expect(invalidated(queryClient, queryKeys.conversationsRoot)).toBe(true)
    expect(invalidated(queryClient, queryKeys.smartMailboxes)).toBe(true)
  })

  it('skips rows + counts on a keyword-only change', () => {
    seed(queryClient)

    applyDomainEvent(queryClient, messageUpdated({ keywords: true }))

    expect(invalidated(queryClient, queryKeys.messagesRoot)).toBe(false)
    expect(invalidated(queryClient, queryKeys.mailboxes('primary'))).toBe(false)
    expect(invalidated(queryClient, queryKeys.conversationsRoot)).toBe(true)
    expect(invalidated(queryClient, queryKeys.smartMailboxes)).toBe(true)
  })

  it('skips rows + counts on a deletion', () => {
    seed(queryClient)

    const event = messageUpdated({})
    event.payload = { deleted: true }
    applyDomainEvent(queryClient, event)

    expect(invalidated(queryClient, queryKeys.messagesRoot)).toBe(false)
    expect(invalidated(queryClient, queryKeys.mailboxes('primary'))).toBe(false)
    expect(invalidated(queryClient, queryKeys.conversationsRoot)).toBe(true)
    expect(invalidated(queryClient, queryKeys.smartMailboxes)).toBe(true)
  })
})
