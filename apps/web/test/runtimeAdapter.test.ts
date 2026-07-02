import { afterEach, describe, expect, it, spyOn } from 'bun:test'

import * as apiClient from '../src/api/client'
import type {
  ConversationView,
  MessageCommand,
  MessageDetail,
  MessagePage,
} from '../src/api/types'
import { messagePageClient } from '../src/messagePageClient'
import type { OperationContext } from '../src/observability'
import {
  getRuntimeAdapter,
  resetRuntimeAdapterForTesting,
  runtimeAdapterForMode,
  setRuntimeAdapterForTesting,
} from '../src/runtime/adapter'
import { createFakeRuntimeAdapter } from '../src/runtime/fakeAdapter'
import { httpRuntimeAdapter } from '../src/runtime/httpAdapter'
import { runtimeMutations } from '../src/runtime/mutations'
import { runtimeSubscriptions } from '../src/runtime/subscriptions'
import { runtimeViews } from '../src/runtime/views'
import type {
  RuntimeMailListViewState,
  RuntimeViewSnapshot,
} from '../src/runtime/types'

const emptyPage: MessagePage = {
  items: [],
  nextCursor: null,
}

const emptyMailListState: RuntimeMailListViewState = {
  scope: null,
  projectionKind: 'message',
  sort: null,
  windowRequest: null,
  rows: [],
  continuation: {
    beforeCursor: null,
    afterCursor: null,
    hasBefore: false,
    hasAfter: false,
  },
  readWatermark: null,
  coverage: { kind: 'complete' },
  knownTotalCount: 0,
  pendingMutations: [],
  anchor: null,
}

const emptyMailListSnapshot: RuntimeViewSnapshot<RuntimeMailListViewState> = {
  viewId: 'view-1',
  descriptor: { family: 'mailList', payload: {} },
  revision: 1,
  lifecycle: 'ready',
  readWatermark: null,
  coverage: { kind: 'complete' },
  data: emptyMailListState,
  pendingMutations: [],
  error: null,
}

const operation: OperationContext = {
  operationId: 'op_1',
  operationKind: 'mail.list',
  operationSource: 'test',
  sessionId: 'session_1',
}

const detail: MessageDetail = {
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
  bodyHtml: null,
  bodyText: null,
  rawMessage: null,
  attachments: [],
}

const conversation: ConversationView = {
  id: 'c1',
  subject: 'Subject',
  messages: [detail],
}

afterEach(() => {
  resetRuntimeAdapterForTesting()
})

describe('runtime adapter facade', () => {
  it('defaults to the HTTP runtime adapter', () => {
    expect(getRuntimeAdapter()).toBe(httpRuntimeAdapter)
  })

  it('selects the HTTP runtime adapter for loopback mode', () => {
    expect(runtimeAdapterForMode(undefined)).toBe(httpRuntimeAdapter)
    expect(runtimeAdapterForMode('loopback')).toBe(httpRuntimeAdapter)
  })

  it('fails closed for native mode until the native adapter exists', async () => {
    const nativeAdapter = runtimeAdapterForMode('native')
    await expect(nativeAdapter.fetchSmartMailboxes()).rejects.toThrow(
      'runtime adapter mode native is not implemented',
    )
    expect(() => nativeAdapter.fetchOAuthRedirectUri()).toThrow(
      'runtime adapter mode native is not implemented',
    )
  })

  it('routes mailbox-membership commands through the named-mutation pipeline', async () => {
    const fake = createFakeRuntimeAdapter()
    setRuntimeAdapterForTesting(fake)

    // Post-M5 the typed MailOperation vocabulary covers every command kind:
    // addToMailbox/removeFromMailbox route through runMutation like the rest —
    // there is no legacy adapter fallback.
    const command: MessageCommand = {
      kind: 'addToMailbox',
      mailboxId: 'archive',
    }
    const result = await runtimeMutations.messages.command({
      command,
      messageId: 'm1',
      sourceId: 'primary',
    })

    expect(result.events).toEqual([])
    expect(fake.messageCommandCalls).toEqual([])
    expect(fake.runtimeMutationCalls).toHaveLength(1)
    expect(fake.runtimeMutationCalls[0].request).toMatchObject({
      name: 'message.addToMailbox',
      args: { sourceId: 'primary', messageId: 'm1', mailboxId: 'archive' },
    })
  })

  it('dispatches message detail reads through a fake adapter override without a backend', async () => {
    const fake = createFakeRuntimeAdapter()
    fake.queueConversation(conversation)
    fake.queueMessage(detail)
    setRuntimeAdapterForTesting(fake)

    expect(await runtimeViews.mail.conversation('c1')).toBe(conversation)
    expect(await runtimeViews.mail.message('m1', 'primary')).toBe(detail)
    expect(fake.conversationCalls).toEqual(['c1'])
    expect(fake.messageCalls).toEqual([
      { messageId: 'm1', sourceId: 'primary' },
    ])
  })

  it('dispatches message page reads through a fake adapter override without a backend', async () => {
    const fake = createFakeRuntimeAdapter()
    fake.queueMessagePage(emptyPage)
    setRuntimeAdapterForTesting(fake)

    const request = {
      scope: {
        kind: 'source-mailbox' as const,
        sourceId: 'primary',
        mailboxId: 'inbox',
      },
      cursor: null,
      limit: 25,
      operation,
      sort: 'date' as const,
      sortDir: 'desc' as const,
    }
    const result = await runtimeViews.mail.messagePage(request)

    expect(result).toBe(emptyPage)
    expect(fake.messagePageCalls).toEqual([request])
  })

  it('dispatches mail-list view opens and frames through a fake adapter override', async () => {
    const fake = createFakeRuntimeAdapter()
    const result = { viewId: 'view-1', snapshot: emptyMailListSnapshot }
    fake.queueOpenMessageListView(result)
    setRuntimeAdapterForTesting(fake)

    const request = {
      scope: {
        kind: 'source-mailbox' as const,
        sourceId: 'primary',
        mailboxId: 'inbox',
      },
      cursor: null,
      limit: 25,
      operation,
      sort: 'date' as const,
      sortDir: 'desc' as const,
    }
    expect(await getRuntimeAdapter().openMessageListView(request)).toBe(result)

    const frames: unknown[] = []
    const unsubscribe = runtimeSubscriptions.view(
      { viewId: 'view-1', afterRevision: 1 },
      { onFrame: (frame) => frames.push(frame) },
    )
    fake.emitViewFrame({ kind: 'replace', snapshot: emptyMailListSnapshot })
    unsubscribe()
    fake.emitViewFrame({ kind: 'closed', viewId: 'view-1' })

    expect(fake.viewOpenCalls).toEqual([request])
    expect(fake.viewSubscriptionCalls).toEqual([
      { request: { viewId: 'view-1', afterRevision: 1 } },
    ])
    expect(frames).toEqual([
      { kind: 'replace', snapshot: emptyMailListSnapshot },
    ])
  })

  it('dispatches navigation reads through a fake adapter override without a backend', async () => {
    const fake = createFakeRuntimeAdapter()
    const readResponse = { results: {} }
    fake.queueReadResponse(readResponse)
    fake.queueMailboxes([])
    fake.queueSmartMailboxes([])
    setRuntimeAdapterForTesting(fake)

    const readRequest = {
      calls: [{ id: 'accounts', op: 'Account/list' as const }],
    }

    expect(await runtimeViews.mail.read(readRequest)).toBe(readResponse)
    expect(await runtimeViews.mail.mailboxes('primary')).toEqual([])
    expect(await runtimeViews.smartMailboxes.list()).toEqual([])
    expect(fake.readCalls).toEqual([readRequest])
    expect(fake.mailboxCalls).toEqual(['primary'])
    expect(fake.smartMailboxCalls).toBe(1)
  })

  it('keeps messagePageClient as a compatibility wrapper over runtime views', async () => {
    const fake = createFakeRuntimeAdapter()
    fake.queueMessagePage(emptyPage)
    setRuntimeAdapterForTesting(fake)

    const result = await messagePageClient.fetchPage({
      scope: { kind: 'global' },
      query: 'from:alex',
      cursor: null,
      limit: 10,
      operation,
    })

    expect(result).toBe(emptyPage)
    expect(fake.messagePageCalls).toHaveLength(1)
    expect(fake.messagePageCalls[0]?.scope).toEqual({ kind: 'global' })
  })

  it('restores the HTTP runtime adapter after a test override', () => {
    setRuntimeAdapterForTesting(createFakeRuntimeAdapter())

    resetRuntimeAdapterForTesting()

    expect(getRuntimeAdapter()).toBe(httpRuntimeAdapter)
  })

  it('restores the previous adapter from an override cleanup', () => {
    const firstFake = createFakeRuntimeAdapter()
    const secondFake = createFakeRuntimeAdapter()
    const restoreFirst = setRuntimeAdapterForTesting(firstFake)
    const restoreSecond = setRuntimeAdapterForTesting(secondFake)

    expect(getRuntimeAdapter()).toBe(secondFake)
    restoreSecond()
    expect(getRuntimeAdapter()).toBe(firstFake)
    restoreFirst()
    expect(getRuntimeAdapter()).toBe(httpRuntimeAdapter)
  })

  it('wraps existing HTTP message detail reads by default', async () => {
    const conversationSpy = spyOn(
      apiClient,
      'fetchConversation',
    ).mockResolvedValue(conversation)
    const messageSpy = spyOn(apiClient, 'fetchMessage').mockResolvedValue(
      detail,
    )

    try {
      expect(await runtimeViews.mail.conversation('c1')).toBe(conversation)
      expect(await runtimeViews.mail.message('m1', 'primary')).toBe(detail)
      expect(conversationSpy).toHaveBeenCalledWith('c1')
      expect(messageSpy).toHaveBeenCalledWith('m1', 'primary')
    } finally {
      conversationSpy.mockRestore()
      messageSpy.mockRestore()
    }
  })

  it('wraps existing HTTP read calls by default', async () => {
    const readSpy = spyOn(apiClient, 'read').mockResolvedValue({ results: {} })

    try {
      const request = {
        calls: [{ id: 'accounts', op: 'Account/list' as const }],
      }
      const result = await runtimeViews.mail.read(request)

      expect(result).toEqual({ results: {} })
      expect(readSpy).toHaveBeenCalledWith(request)
    } finally {
      readSpy.mockRestore()
    }
  })

  it('wraps HTTP mail-list view opens with an intent descriptor by default', async () => {
    const openSpy = spyOn(apiClient, 'openView').mockResolvedValue({
      viewId: 'view-1',
      snapshot: emptyMailListSnapshot,
    })

    try {
      const result = await getRuntimeAdapter().openMessageListView({
        scope: {
          kind: 'source-mailbox',
          sourceId: 'primary',
          mailboxId: 'inbox',
        },
        query: 'from:alex',
        cursor: null,
        limit: 25,
        operation,
        sort: 'date',
        sortDir: 'desc',
      })

      expect(result).toEqual({
        viewId: 'view-1',
        snapshot: emptyMailListSnapshot,
      })
      expect(openSpy).toHaveBeenCalledWith(
        {
          descriptor: {
            family: 'mailList',
            payload: {
              query: 'in:primary/inbox from:alex',
              presentation: {
                kind: 'messages',
                limit: 25,
                cursor: null,
                sortField: 'date',
                sortDirection: 'desc',
              },
              visibility: null,
            },
            clientSelfMaintained: true,
          },
        },
        { sourceId: 'primary' },
      )
    } finally {
      openSpy.mockRestore()
    }
  })

  it('wraps existing HTTP source message reads by default', async () => {
    const pageSpy = spyOn(apiClient, 'fetchSourceMessages').mockResolvedValue(
      emptyPage,
    )

    try {
      const result = await runtimeViews.mail.messagePage({
        scope: { kind: 'source-mailbox', sourceId: 'primary', mailboxId: null },
        query: 'subject:test',
        cursor: 'cursor-1',
        limit: 50,
        operation,
        sort: 'relevance',
        sortDir: 'desc',
      })

      expect(result).toBe(emptyPage)
      expect(pageSpy).toHaveBeenCalledWith('primary', null, {
        q: 'subject:test',
        cursor: 'cursor-1',
        limit: 50,
        sort: undefined,
        sortDir: 'desc',
        signal: undefined,
        operation,
      })
    } finally {
      pageSpy.mockRestore()
    }
  })
})
