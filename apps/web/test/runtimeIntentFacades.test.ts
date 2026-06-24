import { afterEach, describe, expect, it } from 'bun:test'

import type { AccountOverview, DomainEvent } from '../src/api/types'
import {
  resetRuntimeAdapterForTesting,
  setRuntimeAdapterForTesting,
} from '../src/runtime/adapter'
import { createFakeRuntimeAdapter } from '../src/runtime/fakeAdapter'
import { runtimeMutations } from '../src/runtime/mutations'
import { runtimeResources } from '../src/runtime/resources'
import { runtimeStream } from '../src/runtime/runtimeStream'
import { resetRuntimeSessionClientForTesting } from '../src/runtime/sessionClient'
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
  runtime: {
    status: 'ready',
    push: 'disabled',
    lastSyncAt: null,
    lastSyncError: null,
    lastSyncErrorCode: null,
    syncProgress: null,
  },
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
  resetRuntimeSessionClientForTesting()
  resetRuntimeAdapterForTesting()
})

describe('runtime intent facades', () => {
  it('route views, mutations, resources, and subscriptions through the active adapter', async () => {
    const fake = createFakeRuntimeAdapter()
    const resourceBlob = new Blob(['resource'])
    const receivedEvents: DomainEvent[] = []
    const receivedRuntimeFrames: Array<{ type: string; sessionSeq: number }> =
      []
    fake.queueAccounts([account])
    fake.queueMessageCommandResult({ detail: null, events: [] })
    fake.queueResourceBlob(resourceBlob)
    fake.queueRuntimeSession({ sessionId: 'session-1' })
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
    const session = await runtimeStream.openSession({ sourceId: 'primary' })
    const unsubscribe = runtimeSubscriptions.events(
      { afterSeq: 7 },
      { onEvent: (payload) => receivedEvents.push(payload) },
    )
    const unsubscribeRuntime = runtimeStream.subscribe(
      { sessionId: session.sessionId, afterSeq: 1, sourceId: 'primary' },
      { onFrame: (payload) => receivedRuntimeFrames.push(payload) },
    )
    fake.emitDomainEvent(event)
    fake.emitRuntimeFrame({ type: 'heartbeat', sessionSeq: 2 })
    unsubscribe()
    unsubscribeRuntime()

    expect(fake.accountCalls).toBe(1)
    expect(fake.runtimeMutationCalls).toHaveLength(2)
    expect(fake.runtimeMutationCalls[0].request).toMatchObject({
      sessionId: 'session-1',
      sourceId: 'primary',
      name: 'message.setKeywords',
      args: {
        sourceId: 'primary',
        messageId: 'm1',
        command: { add: ['seen'], remove: [] },
      },
    })
    expect(
      fake.runtimeMutationCalls[0].request.clientMutationId.startsWith(
        'client_mutation_',
      ),
    ).toBe(true)
    // Role moves now route through the named-mutation pipeline too.
    expect(fake.runtimeMutationCalls[1].request).toMatchObject({
      sessionId: 'session-1',
      sourceId: 'primary',
      name: 'message.moveToRole',
      args: { sourceId: 'primary', messageId: 'm1', role: 'archive' },
    })
    expect(fake.messageCommandCalls).toEqual([])
    expect(fake.messageRoleMoveCalls).toEqual([])
    expect(fake.resourceCalls).toEqual([
      { descriptor: { kind: 'account-logo', imageId: 'logo-1' } },
    ])
    expect(fake.eventSubscriptionCalls).toEqual([{ request: { afterSeq: 7 } }])
    expect(fake.runtimeSessionCalls).toEqual([
      // The mail-list facade opens its session through the session client, which
      // opts into deltas; the raw openSession below does not.
      { sourceId: 'primary', viewDelta: true },
      { sourceId: 'primary' },
    ])
    expect(fake.runtimeFrameSubscriptionCalls).toEqual([
      { request: { sessionId: 'session-1', afterSeq: 1, sourceId: 'primary' } },
    ])
    expect(receivedEvents).toEqual([event])
    expect(receivedRuntimeFrames).toEqual([
      { type: 'heartbeat', sessionSeq: 2 },
    ])
  })
})
