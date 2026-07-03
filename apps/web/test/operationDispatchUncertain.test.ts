import { afterEach, describe, expect, it } from 'bun:test'

import type { DomainEvent } from '../src/api/types'
import { applyDomainEvent } from '../src/domainCache'
import {
  clearNotifications,
  getNotificationsSnapshot,
} from '../src/notifications/store'
import { createQueryClient } from './domainCache.fixtures'

function dispatchUncertainEvent(reason: string): DomainEvent {
  return {
    seq: 1,
    accountId: 'primary',
    topic: 'operation.dispatch_uncertain',
    occurredAt: '2026-07-03T00:00:00Z',
    mailboxId: null,
    messageId: null,
    payload: { id: 'op-1', reason },
  }
}

describe('operation.dispatch_uncertain handling', () => {
  afterEach(() => {
    clearNotifications()
  })

  // spec: docs/eph/RFC-L2-provider-reliability#32-send-exactly-once
  it('raises a needs-attention notification for a parked send', () => {
    const queryClient = createQueryClient()
    applyDomainEvent(
      queryClient,
      dispatchUncertainEvent('send timed out; delivery uncertain'),
    )

    const notifications = getNotificationsSnapshot()
    expect(notifications).toHaveLength(1)
    expect(notifications[0].severity).toBe('warning')
    expect(notifications[0].message).toContain('send timed out')
    expect(notifications[0].message).toContain('Outbox')
  })

  // A repeated fact for the same parked op collapses to one notification.
  it('deduplicates repeated dispatch-uncertain facts for the same op', () => {
    const queryClient = createQueryClient()
    applyDomainEvent(queryClient, dispatchUncertainEvent('first'))
    applyDomainEvent(queryClient, dispatchUncertainEvent('second'))

    expect(getNotificationsSnapshot()).toHaveLength(1)
  })
})
