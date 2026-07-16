import { describe, expect, it } from 'bun:test'
import type { InfiniteData } from '@tanstack/react-query'
import { QueryClient } from '@tanstack/react-query'

import type { MessagePage, MessageSummary } from '../src/api/types'
import { findConversationIdForMessage } from '../src/mailState'
import { queryKeys } from '../src/queryKeys'

function createQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: { gcTime: Number.POSITIVE_INFINITY, retry: false },
    },
  })
}

function messageSummary(
  overrides: Partial<MessageSummary> = {},
): MessageSummary {
  return {
    id: 'm1',
    sourceId: 'primary',
    sourceName: 'Primary',
    sourceThreadId: 't1',
    conversationId: 'c1',
    subject: 'Subject',
    fromName: 'Sender',
    fromEmail: 'sender@example.com',
    to: [],
    preview: 'preview',
    receivedAt: '2026-04-28T12:00:00Z',
    hasAttachment: false,
    isRead: false,
    isFlagged: false,
    mailboxIds: ['inbox'],
    keywords: [],
    ...overrides,
  }
}

function seedListView(queryClient: QueryClient, items: MessageSummary[]) {
  const data: InfiniteData<MessagePage, string | null> = {
    pageParams: [null],
    pages: [{ items, nextCursor: null }],
  }
  queryClient.setQueryData(
    queryKeys.messages(
      { kind: 'source-mailbox', sourceId: 'primary', mailboxId: 'inbox' },
      undefined,
      undefined,
    ),
    data,
  )
}

describe('findConversationIdForMessage', () => {
  it('resolves the conversation id from a message-list page', () => {
    const queryClient = createQueryClient()
    seedListView(queryClient, [
      messageSummary({ conversationId: 'c-from-list' }),
    ])
    expect(
      findConversationIdForMessage(queryClient, {
        sourceId: 'primary',
        messageId: 'm1',
      }),
    ).toBe('c-from-list')
  })
})
