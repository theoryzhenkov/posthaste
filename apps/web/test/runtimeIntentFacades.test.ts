import { afterEach, describe, expect, it } from 'bun:test'

import type { AccountOverview, DomainEvent } from '../src/api/types'
import {
  resetRuntimeAdapterForTesting,
  setRuntimeAdapterForTesting,
} from '../src/runtime/adapter'
import { createFakeRuntimeAdapter } from '../src/runtime/fakeAdapter'
import { runtimeMutations } from '../src/runtime/mutations'
import { runtimeResources } from '../src/runtime/resources'
import { runtimeSubscriptions } from '../src/runtime/subscriptions'
import { runtimeViews } from '../src/runtime/views'

const account: AccountOverview = {
  id: 'primary',
  name: 'Primary',
  fullName: null,
  emailPatterns: ['primary@example.com'],
  driver: 'mock',
  enabled: true,
  appearance: { kind: 'initials', initials: 'P', colorHue: 200 },
  connection: {
    kind: 'manualCredentials',
    provider: 'generic',
    providerKind: 'generic',
    auth: 'password',
    baseUrl: null,
    username: 'primary@example.com',
    imap: null,
    smtp: null,
    secret: { storage: 'os', configured: true, label: null },
  },
  createdAt: '2026-04-28T12:00:00Z',
  updatedAt: '2026-04-28T12:00:00Z',
  isDefault: true,
  status: 'ready',
  push: 'disabled',
  lastSyncAt: null,
  lastSyncError: null,
  lastSyncErrorCode: null,
  syncProgress: null,
}

const event: DomainEvent = {
  seq: 1,
  accountId: 'primary',
  topic: 'message.updated',
  occurredAt: '2026-04-28T12:00:00Z',
  mailboxId: null,
  messageId: 'm1',
  payload: {},
}

afterEach(() => {
  resetRuntimeAdapterForTesting()
})

describe('runtime intent facades', () => {
  it('route views, mutations, resources, and subscriptions through the active adapter', async () => {
    const fake = createFakeRuntimeAdapter()
    const resourceBlob = new Blob(['resource'])
    const receivedEvents: DomainEvent[] = []
    fake.queueAccounts([account])
    fake.queueMessageCommandResult({ detail: null, events: [] })
    fake.queueMessageCommandResult({ detail: null, events: [] })
    fake.queueResourceBlob(resourceBlob)
    setRuntimeAdapterForTesting(fake)

    expect(await runtimeViews.accounts.list()).toEqual([account])
    await runtimeMutations.messages.command({
      sourceId: 'primary',
      messageId: 'm1',
      command: { kind: 'setKeywords', add: ['seen'], remove: [] },
    })
    await runtimeMutations.messages.moveToMailboxRole({
      sourceId: 'primary',
      messageId: 'm1',
      role: 'archive',
    })
    expect(
      await runtimeResources.blob({ kind: 'account-logo', imageId: 'logo-1' }),
    ).toBe(resourceBlob)
    const unsubscribe = runtimeSubscriptions.events(
      { afterSeq: 7 },
      { onEvent: (payload) => receivedEvents.push(payload) },
    )
    fake.emitDomainEvent(event)
    unsubscribe()

    expect(fake.accountCalls).toBe(1)
    expect(fake.messageCommandCalls).toEqual([
      {
        sourceId: 'primary',
        messageId: 'm1',
        command: { kind: 'setKeywords', add: ['seen'], remove: [] },
      },
    ])
    expect(fake.messageRoleMoveCalls).toEqual([
      { sourceId: 'primary', messageId: 'm1', role: 'archive' },
    ])
    expect(fake.resourceCalls).toEqual([
      { descriptor: { kind: 'account-logo', imageId: 'logo-1' } },
    ])
    expect(fake.eventSubscriptionCalls).toEqual([{ request: { afterSeq: 7 } }])
    expect(receivedEvents).toEqual([event])
  })
})
