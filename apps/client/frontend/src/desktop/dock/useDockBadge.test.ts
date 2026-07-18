import { describe, expect, test } from 'bun:test'

import type { MailboxCountsRow } from '@/gen'
import { inboxUnreadTotal } from './useDockBadge'

function countRow(
  accountId: string,
  role: string | null,
  unreadEmails: number,
): MailboxCountsRow {
  return {
    accountId,
    mailbox: {
      id: `${accountId}-${role ?? 'folder'}`,
      name: role ?? 'Folder',
      role,
      unreadEmails,
      totalEmails: unreadEmails,
    },
  }
}

describe('inboxUnreadTotal', () => {
  test('sums inbox-role unread for the given accounts only', () => {
    const rows = [
      countRow('a', 'inbox', 3),
      countRow('a', 'junk', 9),
      countRow('b', 'inbox', 2),
      countRow('c', 'inbox', 7), // not in the account set (disabled)
      countRow('a', null, 5),
    ]
    expect(inboxUnreadTotal(rows, new Set(['a', 'b']))).toBe(5)
  })

  test('empty rows yield zero', () => {
    expect(inboxUnreadTotal([], new Set(['a']))).toBe(0)
  })
})
