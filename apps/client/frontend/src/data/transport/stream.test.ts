import { describe, expect, test } from 'bun:test'

import type { DomainEventPayload, MessageUpdatedPayload } from '@/gen'

import { parseMessageUpdated } from './stream'

const diffPayload: MessageUpdatedPayload = {
  messageId: 'm1',
  sourceThreadId: 't1',
  conversationId: 'c1',
  created: true,
  changes: { keywords: true, mailboxes: true, arrived: true },
  keywords: [],
  mailboxIds: ['inbox'],
  arrivedMailboxIds: ['inbox'],
}

function event(overrides: Partial<DomainEventPayload> = {}): DomainEventPayload {
  return {
    kind: 'message.updated',
    accountId: 'acct-1',
    payload: diffPayload,
    ...overrides,
  }
}

describe('parseMessageUpdated', () => {
  test('the sync-projection diff shape parses', () => {
    expect(parseMessageUpdated(event())).toEqual(diffPayload)
  })

  test('a non-created diff (move of an existing message) still parses', () => {
    const moved = { ...diffPayload, created: false }
    expect(parseMessageUpdated(event({ payload: moved }))).toEqual(moved)
  })

  test('other event kinds never parse', () => {
    expect(
      parseMessageUpdated(event({ kind: 'operation.settled' })),
    ).toBeNull()
    expect(parseMessageUpdated(event({ kind: 'sync.completed' }))).toBeNull()
  })

  test('a payload-less event never parses', () => {
    expect(
      parseMessageUpdated({ kind: 'message.updated', accountId: 'acct-1' }),
    ).toBeNull()
  })

  test('narrower message.updated shapes (no created) never parse', () => {
    // The settle-revert echo: flags both dimensions, states no `created`.
    const reverted = {
      messageId: 'm1',
      changes: { keywords: true, mailboxes: true },
      reverted: true,
    }
    expect(parseMessageUpdated(event({ payload: reverted }))).toBeNull()
    // The deletion echo.
    expect(
      parseMessageUpdated(
        event({ payload: { messageId: 'm1', deleted: true } }),
      ),
    ).toBeNull()
  })
})
