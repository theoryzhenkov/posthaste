import { describe, expect, it } from 'bun:test'
import type { InfiniteData } from '@tanstack/react-query'
import { QueryClient } from '@tanstack/react-query'

import type { DomainEvent, MessagePage, MessageSummary } from '../src/api/types'
import { applyDomainEvent } from '../src/domainCache'
import { EVENT_TOPICS } from '../src/domainVocabulary'
import {
  applyKeywordPatch,
  deriveKeywordState,
  type MailSelection,
} from '../src/mailState'
import { queryKeys } from '../src/queryKeys'

function createQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: {
        gcTime: Number.POSITIVE_INFINITY,
        retry: false,
      },
    },
  })
}

function messageSummary(
  overrides: Partial<MessageSummary> = {},
): MessageSummary {
  return {
    id: 'message-1',
    sourceId: 'primary',
    sourceName: 'Primary',
    sourceThreadId: 'thread-1',
    conversationId: 'conversation-1',
    subject: 'Project update',
    fromName: 'A Sender',
    fromEmail: 'sender@example.com',
    preview: 'Here is the update',
    receivedAt: '2026-04-28T12:00:00Z',
    hasAttachment: false,
    isRead: false,
    isFlagged: false,
    mailboxIds: ['inbox'],
    keywords: [],
    ...overrides,
  }
}

function domainEvent(overrides: Partial<DomainEvent> = {}): DomainEvent {
  return {
    seq: 1,
    accountId: 'primary',
    topic: EVENT_TOPICS.MessageArrived,
    occurredAt: '2026-04-28T12:01:00Z',
    mailboxId: 'inbox',
    messageId: 'message-1',
    payload: {},
    ...overrides,
  }
}

function seedMessageList(
  queryClient: QueryClient,
  queryKey: ReturnType<typeof queryKeys.messages>,
  message: MessageSummary,
) {
  queryClient.setQueryData<InfiniteData<MessagePage, string | null>>(queryKey, {
    pageParams: [null],
    pages: [{ items: [message], nextCursor: null }],
  })
}

function cachedMessage(
  queryClient: QueryClient,
  queryKey: ReturnType<typeof queryKeys.messages>,
) {
  return queryClient.getQueryData<InfiniteData<MessagePage, string | null>>(
    queryKey,
  )?.pages[0]?.items[0]
}

describe('frontend domain cache contracts', () => {
  // spec: docs/L0-testing#frontend-state-contracts
  it('keeps optimistic keyword changes visible across mailbox smart mailbox and tag views', () => {
    const queryClient = createQueryClient()
    const message = messageSummary()
    const mailboxView = queryKeys.messages({
      kind: 'source-mailbox',
      sourceId: 'primary',
      mailboxId: 'inbox',
    })
    const smartMailboxView = queryKeys.messages({
      kind: 'smart-mailbox',
      id: 'sm-work',
    })
    const tagView = queryKeys.messages(
      {
        kind: 'source-mailbox',
        sourceId: 'primary',
        mailboxId: 'inbox',
      },
      'tag:work',
    )
    seedMessageList(queryClient, mailboxView, message)
    seedMessageList(queryClient, smartMailboxView, message)
    seedMessageList(queryClient, tagView, message)

    const selection: MailSelection = {
      conversationId: 'conversation-1',
      sourceId: 'primary',
      messageId: 'message-1',
    }
    applyKeywordPatch(queryClient, selection, {
      previous: deriveKeywordState([]),
      next: deriveKeywordState(['work']),
    })

    expect(cachedMessage(queryClient, mailboxView)?.keywords).toEqual(['work'])
    expect(cachedMessage(queryClient, smartMailboxView)?.keywords).toEqual([
      'work',
    ])
    expect(cachedMessage(queryClient, tagView)?.keywords).toEqual(['work'])
  })

  // spec: docs/L0-testing#frontend-state-contracts
  it('invalidates message list views when a remote event can change visible rows', () => {
    const queryClient = createQueryClient()
    const messageList = queryKeys.messages({
      kind: 'source-mailbox',
      sourceId: 'primary',
      mailboxId: 'inbox',
    })
    seedMessageList(queryClient, messageList, messageSummary())

    applyDomainEvent(queryClient, domainEvent())

    expect(queryClient.getQueryState(messageList)?.isInvalidated).toBe(true)
  })

  // spec: docs/L0-testing#frontend-state-contracts
  it('invalidates mailbox read models when a mailbox changes remotely', () => {
    const queryClient = createQueryClient()
    const mailboxList = queryKeys.mailboxes('primary')
    const messageList = queryKeys.messages({
      kind: 'source-mailbox',
      sourceId: 'primary',
      mailboxId: 'inbox',
    })
    queryClient.setQueryData(queryKeys.sidebar, { accounts: [] })
    queryClient.setQueryData(mailboxList, [])
    queryClient.setQueryData(queryKeys.smartMailboxes, [])
    seedMessageList(queryClient, messageList, messageSummary())

    applyDomainEvent(
      queryClient,
      domainEvent({
        topic: EVENT_TOPICS.MailboxUpdated,
        messageId: null,
        payload: { mailboxId: 'inbox' },
      }),
    )

    expect(queryClient.getQueryState(queryKeys.sidebar)?.isInvalidated).toBe(
      true,
    )
    expect(queryClient.getQueryState(mailboxList)?.isInvalidated).toBe(true)
    expect(
      queryClient.getQueryState(queryKeys.smartMailboxes)?.isInvalidated,
    ).toBe(true)
    expect(queryClient.getQueryState(messageList)?.isInvalidated).toBe(true)
  })
})
