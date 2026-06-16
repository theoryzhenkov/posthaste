import { afterEach, describe, expect, it, spyOn } from 'bun:test'

import * as apiClient from '../src/api/client'
import type {
  ConversationView,
  MessageCommand,
  MessageCommandResult,
  MessageDetail,
  MessagePage,
} from '../src/api/types'
import { messagePageClient } from '../src/messagePageClient'
import type { OperationContext } from '../src/observability'
import {
  fetchRuntimeConversation,
  fetchRuntimeMailboxes,
  fetchRuntimeMessage,
  fetchRuntimeMessagePage,
  fetchRuntimeSmartMailboxes,
  getRuntimeAdapter,
  resetRuntimeAdapterForTesting,
  runRuntimeMessageCommand,
  runtimeAdapterForMode,
  runtimeRead,
  setRuntimeAdapterForTesting,
} from '../src/runtime/adapter'
import { createFakeRuntimeAdapter } from '../src/runtime/fakeAdapter'
import { httpRuntimeAdapter } from '../src/runtime/httpAdapter'

const command: MessageCommand = {
  kind: 'replaceMailboxes',
  mailboxIds: ['archive'],
}

const okResult: MessageCommandResult = {
  detail: null,
  events: [],
}

const emptyPage: MessagePage = {
  items: [],
  nextCursor: null,
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

  it('dispatches message commands through a fake adapter override without a backend', async () => {
    const fake = createFakeRuntimeAdapter()
    fake.queueMessageCommandResult(okResult)
    setRuntimeAdapterForTesting(fake)

    const result = await runRuntimeMessageCommand({
      command,
      messageId: 'm1',
      sourceId: 'primary',
    })

    expect(result).toBe(okResult)
    expect(fake.messageCommandCalls).toEqual([
      {
        command,
        messageId: 'm1',
        sourceId: 'primary',
      },
    ])
  })

  it('dispatches message detail reads through a fake adapter override without a backend', async () => {
    const fake = createFakeRuntimeAdapter()
    fake.queueConversation(conversation)
    fake.queueMessage(detail)
    setRuntimeAdapterForTesting(fake)

    expect(await fetchRuntimeConversation('c1')).toBe(conversation)
    expect(await fetchRuntimeMessage('m1', 'primary')).toBe(detail)
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
    const result = await fetchRuntimeMessagePage(request)

    expect(result).toBe(emptyPage)
    expect(fake.messagePageCalls).toEqual([request])
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

    expect(await runtimeRead(readRequest)).toBe(readResponse)
    expect(await fetchRuntimeMailboxes('primary')).toEqual([])
    expect(await fetchRuntimeSmartMailboxes()).toEqual([])
    expect(fake.readCalls).toEqual([readRequest])
    expect(fake.mailboxCalls).toEqual(['primary'])
    expect(fake.smartMailboxCalls).toBe(1)
  })

  it('keeps messagePageClient as a compatibility wrapper over the runtime adapter', async () => {
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

  it('wraps existing HTTP message command behavior by default', async () => {
    const commandSpy = spyOn(
      apiClient,
      'performMessageCommand',
    ).mockResolvedValue(okResult)

    try {
      const result = await runRuntimeMessageCommand({
        command,
        messageId: 'm1',
        sourceId: 'primary',
      })

      expect(result).toBe(okResult)
      expect(commandSpy).toHaveBeenCalledWith('m1', command, 'primary')
    } finally {
      commandSpy.mockRestore()
    }
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
      expect(await fetchRuntimeConversation('c1')).toBe(conversation)
      expect(await fetchRuntimeMessage('m1', 'primary')).toBe(detail)
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
      const result = await runtimeRead(request)

      expect(result).toEqual({ results: {} })
      expect(readSpy).toHaveBeenCalledWith(request)
    } finally {
      readSpy.mockRestore()
    }
  })

  it('wraps existing HTTP source message reads by default', async () => {
    const pageSpy = spyOn(apiClient, 'fetchSourceMessages').mockResolvedValue(
      emptyPage,
    )

    try {
      const result = await fetchRuntimeMessagePage({
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
