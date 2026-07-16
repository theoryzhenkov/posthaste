import { afterEach, describe, expect, it } from 'bun:test'

import {
  clearNotifications,
  dismissNotification,
  getNotificationsSnapshot,
  markAllNotificationsRead,
  pushNotification,
} from '../src/notifications/store'

const getNotificationsForTest = getNotificationsSnapshot

afterEach(() => {
  clearNotifications()
})

describe('notifications store', () => {
  it('adds notifications newest-first and unread', () => {
    pushNotification({ severity: 'error', title: 'First' })
    pushNotification({ severity: 'info', title: 'Second' })

    const items = getNotificationsForTest()
    expect(items.map((n) => n.title)).toEqual(['Second', 'First'])
    expect(items.every((n) => !n.read)).toBe(true)
  })

  it('collapses repeated notifications by dedupeKey and bumps them unread', () => {
    const firstId = pushNotification({
      severity: 'error',
      title: 'Corrupt',
      dedupeKey: 'storage_corrupted',
    })
    markAllNotificationsRead()
    pushNotification({ severity: 'info', title: 'Other' })

    const secondId = pushNotification({
      severity: 'error',
      title: 'Corrupt again',
      dedupeKey: 'storage_corrupted',
    })

    const items = getNotificationsForTest()
    // Same entry reused, moved to the top, and marked unread again.
    expect(secondId).toBe(firstId)
    expect(items).toHaveLength(2)
    expect(items[0].title).toBe('Corrupt again')
    expect(items[0].read).toBe(false)
  })

  it('dismiss and clear remove notifications', () => {
    const id = pushNotification({ severity: 'error', title: 'Boom' })
    pushNotification({ severity: 'info', title: 'Keep' })

    dismissNotification(id)
    expect(getNotificationsForTest().map((n) => n.title)).toEqual(['Keep'])

    clearNotifications()
    expect(getNotificationsForTest()).toHaveLength(0)
  })
})
