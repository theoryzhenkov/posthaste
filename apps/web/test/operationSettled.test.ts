import { afterEach, describe, expect, it } from 'bun:test'

import type { DomainEvent } from '../src/api/types'
import { applyDomainEvent } from '../src/domainCache'
import {
  clearNotifications,
  getNotificationsSnapshot,
} from '../src/notifications/store'
import { createQueryClient } from './domainCache.fixtures'

function settlementEvent(
  outcome: 'applied' | 'conflicted' | 'failed',
  error: string | null = null,
): DomainEvent {
  return {
    seq: 1,
    accountId: 'primary',
    topic: 'operation.settled',
    occurredAt: '2026-06-21T00:00:00Z',
    mailboxId: null,
    messageId: null,
    payload: { id: 'op-1', outcome, assignedEntityId: null, error },
  }
}

describe('operation.settled handling', () => {
  afterEach(() => {
    clearNotifications()
  })

  // spec: docs/L1-outbox#settlement
  it('notifies on a failed operation settlement', () => {
    const queryClient = createQueryClient()
    applyDomainEvent(
      queryClient,
      settlementEvent('failed', 'provider rejected'),
    )

    const notifications = getNotificationsSnapshot()
    expect(notifications).toHaveLength(1)
    expect(notifications[0].severity).toBe('error')
    expect(notifications[0].message).toBe('provider rejected')
  })

  // spec: docs/L1-outbox#settlement
  it('stays silent on a successful settlement', () => {
    const queryClient = createQueryClient()
    applyDomainEvent(queryClient, settlementEvent('applied'))

    expect(getNotificationsSnapshot()).toHaveLength(0)
  })
})
