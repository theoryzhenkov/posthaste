import { describe, expect, it } from 'bun:test'
import type { InfiniteData } from '@tanstack/react-query'
import { QueryClient } from '@tanstack/react-query'

import type {
  ConversationView,
  MessageDetail,
  MessagePage,
  MessageSummary,
} from '../src/api/types'
import {
  applyMailboxPatch,
  captureMutableState,
  findConversationIdForMessage,
  mailKeys,
  restoreSnapshots,
  type MailSelection,
} from '../src/mailState'
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

function seedListView(
  queryClient: QueryClient,
  selection:
    | { kind: 'source-mailbox'; sourceId: string; mailboxId: string }
    | { kind: 'smart-mailbox'; id: string },
  items: MessageSummary[],
) {
  const data: InfiniteData<MessagePage, string | null> = {
    pageParams: [null],
    pages: [{ items, nextCursor: null }],
  }
  queryClient.setQueryData(
    queryKeys.messages(selection, undefined, undefined),
    data,
  )
}

function listItems(
  queryClient: QueryClient,
  selection:
    | { kind: 'source-mailbox'; sourceId: string; mailboxId: string }
    | { kind: 'smart-mailbox'; id: string },
): MessageSummary[] {
  const data = queryClient.getQueryData<
    InfiniteData<MessagePage, string | null>
  >(queryKeys.messages(selection, undefined, undefined))
  return data?.pages.flatMap((page) => page.items) ?? []
}

const inboxView = {
  kind: 'source-mailbox' as const,
  sourceId: 'primary',
  mailboxId: 'inbox',
}
const selection: MailSelection = {
  sourceId: 'primary',
  messageId: 'm1',
  conversationId: 'c1',
}

describe('captureMutableState', () => {
  it('reads mutable state from a message-list page', () => {
    const queryClient = createQueryClient()
    seedListView(queryClient, inboxView, [
      messageSummary({ keywords: ['$seen'], mailboxIds: ['inbox'] }),
    ])
    expect(
      captureMutableState(queryClient, {
        sourceId: 'primary',
        messageId: 'm1',
      }),
    ).toEqual({ keywords: ['$seen'], mailboxIds: ['inbox'] })
  })

  it('returns null when the message is not cached', () => {
    const queryClient = createQueryClient()
    expect(
      captureMutableState(queryClient, {
        sourceId: 'primary',
        messageId: 'missing',
      }),
    ).toBeNull()
  })
})

describe('applyMailboxPatch', () => {
  it('removes the row from a source-mailbox view it no longer belongs to', () => {
    const queryClient = createQueryClient()
    seedListView(queryClient, inboxView, [
      messageSummary(),
      messageSummary({ id: 'm2', conversationId: 'c2' }),
    ])

    const result = applyMailboxPatch(queryClient, selection, ['archive'])

    expect(result.incomplete).toBe(false)
    expect(listItems(queryClient, inboxView).map((m) => m.id)).toEqual(['m2'])
  })

  it('rolls back exactly via snapshots', () => {
    const queryClient = createQueryClient()
    const original = [messageSummary(), messageSummary({ id: 'm2' })]
    seedListView(queryClient, inboxView, original)

    const result = applyMailboxPatch(queryClient, selection, ['archive'])
    expect(listItems(queryClient, inboxView).map((m) => m.id)).toEqual(['m2'])

    restoreSnapshots(queryClient, result.snapshots)
    expect(listItems(queryClient, inboxView).map((m) => m.id)).toEqual([
      'm1',
      'm2',
    ])
  })

  it('marks the patch incomplete for smart-mailbox views and leaves the row', () => {
    const queryClient = createQueryClient()
    const smartView = { kind: 'smart-mailbox' as const, id: 'unread' }
    seedListView(queryClient, smartView, [messageSummary()])

    const result = applyMailboxPatch(queryClient, selection, ['archive'])

    expect(result.incomplete).toBe(true)
    // Membership is undecidable, so the row stays until server reconciliation.
    expect(listItems(queryClient, smartView).map((m) => m.id)).toEqual(['m1'])
  })

  it('removes the row from every list on destroy', () => {
    const queryClient = createQueryClient()
    seedListView(queryClient, inboxView, [messageSummary()])
    const smartView = { kind: 'smart-mailbox' as const, id: 'unread' }
    seedListView(queryClient, smartView, [messageSummary()])

    applyMailboxPatch(queryClient, selection, [], { destroy: true })

    expect(listItems(queryClient, inboxView)).toEqual([])
    expect(listItems(queryClient, smartView)).toEqual([])
  })

  it('removes detail + conversation summary on destroy and restores them on rollback', () => {
    const queryClient = createQueryClient()
    const detail = {
      ...messageSummary(),
      bodyHtml: null,
      bodyText: null,
      rawMessage: null,
      attachments: [],
    } as MessageDetail
    queryClient.setQueryData(mailKeys.message('primary', 'm1'), detail)
    const conversation: ConversationView = {
      id: 'c1',
      subject: 'Subject',
      messages: [messageSummary()],
    }
    queryClient.setQueryData(mailKeys.conversation('c1'), conversation)
    queryClient.setQueryData(mailKeys.conversationSummary('c1'), {
      id: 'c1',
    })

    const result = applyMailboxPatch(queryClient, selection, [], {
      destroy: true,
    })

    expect(
      queryClient.getQueryData(mailKeys.message('primary', 'm1')),
    ).toBeUndefined()
    expect(
      queryClient.getQueryData(mailKeys.conversationSummary('c1')),
    ).toBeUndefined()

    restoreSnapshots(queryClient, result.snapshots)
    expect(queryClient.getQueryData(mailKeys.message('primary', 'm1'))).toEqual(
      detail,
    )
    expect(
      queryClient.getQueryData(mailKeys.conversationSummary('c1')),
    ).toEqual({ id: 'c1' })
  })
})

describe('captureMutableState conversation fallback', () => {
  it('reads state from a cached conversation view', () => {
    const queryClient = createQueryClient()
    queryClient.setQueryData<ConversationView>(mailKeys.conversation('c1'), {
      id: 'c1',
      subject: 'Subject',
      messages: [
        messageSummary({ keywords: ['$flagged'], mailboxIds: ['archive'] }),
      ],
    })
    expect(
      captureMutableState(queryClient, {
        sourceId: 'primary',
        messageId: 'm1',
      }),
    ).toEqual({ keywords: ['$flagged'], mailboxIds: ['archive'] })
  })
})

describe('findConversationIdForMessage', () => {
  it('resolves the conversation id from a message-list page', () => {
    const queryClient = createQueryClient()
    seedListView(queryClient, inboxView, [
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
