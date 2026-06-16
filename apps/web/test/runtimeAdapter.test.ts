import { afterEach, describe, expect, it, spyOn } from 'bun:test'

import * as apiClient from '../src/api/client'
import type {
  MessageCommand,
  MessageCommandResult,
  MessagePage,
} from '../src/api/types'
import { messagePageClient } from '../src/messagePageClient'
import type { OperationContext } from '../src/observability'
import {
  fetchRuntimeMessagePage,
  getRuntimeAdapter,
  resetRuntimeAdapterForTesting,
  runRuntimeMessageCommand,
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

afterEach(() => {
  resetRuntimeAdapterForTesting()
})

describe('runtime adapter facade', () => {
  it('defaults to the HTTP runtime adapter', () => {
    expect(getRuntimeAdapter()).toBe(httpRuntimeAdapter)
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

  it('dispatches message page reads through a fake adapter override without a backend', async () => {
    const fake = createFakeRuntimeAdapter()
    fake.queueMessagePage(emptyPage)
    setRuntimeAdapterForTesting(fake)

    const request = {
      scope: { kind: 'source-mailbox' as const, sourceId: 'primary', mailboxId: 'inbox' },
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
