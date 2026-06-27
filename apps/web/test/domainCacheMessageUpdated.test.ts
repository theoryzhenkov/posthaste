import { afterEach, beforeEach, describe, expect, it } from 'bun:test'

import { QueryClient } from '@tanstack/react-query'

import type { DomainEvent } from '../src/api/types'
import { applyDomainEvent } from '../src/domainCache'
import { mailKeys } from '../src/mailState'
import { queryKeys } from '../src/queryKeys'
import { setEntityStoreActiveForTesting } from '../src/runtime/entityStoreState'

// `message.updated` is the event the entity store owns when active: it drives
// the mail-list rows (synthesized view frames) + the mailbox counts
// (`setQueryData`). 2e.3 retires the redundant REST invalidations for those
// store-owned keys while keeping the surfaces the store does not own
// (conversations, smart-mailboxes).

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

describe('applyDomainEvent message.updated (2e.3 store-owned invalidation gate)', () => {
  let queryClient: QueryClient
  let restore: () => void

  beforeEach(() => {
    queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    })
    restore = () => undefined
  })

  afterEach(() => {
    restore()
    queryClient.clear()
  })

  it('invalidates rows + counts when the store is NOT active (legacy)', () => {
    restore = setEntityStoreActiveForTesting(false)
    seed(queryClient)

    applyDomainEvent(queryClient, messageUpdated({ arrived: true }))

    expect(invalidated(queryClient, queryKeys.messagesRoot)).toBe(true)
    expect(invalidated(queryClient, queryKeys.mailboxes('primary'))).toBe(true)
    // Non-store surfaces still invalidate.
    expect(invalidated(queryClient, queryKeys.conversationsRoot)).toBe(true)
    expect(invalidated(queryClient, queryKeys.smartMailboxes)).toBe(true)
  })

  it('skips rows + counts when the store IS active (the store owns them)', () => {
    restore = setEntityStoreActiveForTesting(true)
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

  it('skips rows + counts on a keyword-only change when the store is active', () => {
    restore = setEntityStoreActiveForTesting(true)
    seed(queryClient)

    applyDomainEvent(queryClient, messageUpdated({ keywords: true }))

    expect(invalidated(queryClient, queryKeys.messagesRoot)).toBe(false)
    expect(invalidated(queryClient, queryKeys.mailboxes('primary'))).toBe(false)
    expect(invalidated(queryClient, queryKeys.conversationsRoot)).toBe(true)
    expect(invalidated(queryClient, queryKeys.smartMailboxes)).toBe(true)
  })

  it('skips rows + counts on a deletion when the store is active', () => {
    restore = setEntityStoreActiveForTesting(true)
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
