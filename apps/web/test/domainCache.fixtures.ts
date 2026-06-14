import type { InfiniteData } from '@tanstack/react-query'
import { QueryClient } from '@tanstack/react-query'

import type {
  AccountOverview,
  DomainEvent,
  MessagePage,
  MessageSummary,
} from '../src/api/types'
import { EVENT_TOPICS } from '../src/domainVocabulary'
import type { queryKeys } from '../src/queryKeys'

export function createQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: {
        gcTime: Number.POSITIVE_INFINITY,
        retry: false,
      },
    },
  })
}

export function messageSummary(
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
    to: [],
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

export function domainEvent(overrides: Partial<DomainEvent> = {}): DomainEvent {
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

export function accountOverview(
  overrides: Partial<AccountOverview> = {},
): AccountOverview {
  return {
    id: 'primary',
    name: 'Primary',
    fullName: null,
    emailPatterns: ['*@example.com'],
    driver: 'mock',
    enabled: true,
    appearance: { kind: 'initials', initials: 'P', colorHue: 120 },
    connection: {
      kind: 'manualCredentials',
      provider: 'generic',
      providerKind: 'generic',
      auth: 'password',
      username: 'primary@example.com',
      imap: null,
      smtp: null,
      secret: { storage: 'env', configured: false, label: null },
      baseUrl: null,
    },
    createdAt: '2026-04-28T12:00:00Z',
    updatedAt: '2026-04-28T12:00:00Z',
    isDefault: true,
    status: 'ready',
    push: 'connected',
    lastSyncAt: '2026-04-28T12:00:00Z',
    lastSyncError: null,
    lastSyncErrorCode: null,
    syncProgress: null,
    ...overrides,
  }
}

export function seedMessageList(
  queryClient: QueryClient,
  queryKey: ReturnType<typeof queryKeys.messages>,
  message: MessageSummary,
) {
  queryClient.setQueryData<InfiniteData<MessagePage, string | null>>(queryKey, {
    pageParams: [null],
    pages: [{ items: [message], nextCursor: null }],
  })
}

export function cachedMessage(
  queryClient: QueryClient,
  queryKey: ReturnType<typeof queryKeys.messages>,
) {
  return queryClient.getQueryData<InfiniteData<MessagePage, string | null>>(
    queryKey,
  )?.pages[0]?.items[0]
}
