import { afterEach, describe, expect, it } from 'bun:test'

import type { Mailbox } from '../src/api/types'
import {
  resetRuntimeAdapterForTesting,
  setRuntimeAdapterForTesting,
} from '../src/runtime/adapter'
import { createFakeRuntimeAdapter } from '../src/runtime/fakeAdapter'
import { runtimeMutations } from '../src/runtime/mutations'

afterEach(() => {
  resetRuntimeAdapterForTesting()
})

describe('runtimeMutations.mailboxes.create', () => {
  it('routes the create through the active adapter and returns the refreshed list', async () => {
    const created: Mailbox = {
      id: 'mb-Receipts',
      name: 'Receipts',
      role: null,
      unreadEmails: 0,
      totalEmails: 0,
    }
    const calls: Array<{ accountId: string; name: string }> = []

    const fake = createFakeRuntimeAdapter()
    setRuntimeAdapterForTesting({
      ...fake,
      createMailbox: (accountId, input) => {
        calls.push({ accountId, name: input.name })
        return Promise.resolve([created])
      },
    })

    const result = await runtimeMutations.mailboxes.create('primary', {
      name: 'Receipts',
    })

    expect(calls).toEqual([{ accountId: 'primary', name: 'Receipts' }])
    expect(result).toEqual([created])
  })
})
