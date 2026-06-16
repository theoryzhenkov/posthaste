import { afterEach, describe, expect, it, spyOn } from 'bun:test'

import * as apiClient from '../src/api/client'
import type { MessageCommand, MessageCommandResult } from '../src/api/types'
import {
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

afterEach(() => {
  resetRuntimeAdapterForTesting()
})

describe('runtime adapter facade', () => {
  it('defaults to the HTTP runtime adapter', () => {
    expect(getRuntimeAdapter()).toBe(httpRuntimeAdapter)
  })

  it('dispatches through a fake adapter override without a backend', async () => {
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
})
