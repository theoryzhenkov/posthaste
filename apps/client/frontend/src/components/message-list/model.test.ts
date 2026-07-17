import { describe, expect, test } from 'bun:test'

import { prepareServerSearchQuery } from '@/searchQuery'
import { buildMailListQuery } from './model'

describe('buildMailListQuery', () => {
  test('source-mailbox view scopes by account and mailbox', () => {
    const query = buildMailListQuery(
      {
        kind: 'source-mailbox',
        sourceId: 'acct-1',
        mailboxId: 'mb-1',
        name: 'Inbox',
      },
      prepareServerSearchQuery(undefined),
      { columnId: 'date', direction: 'desc' },
    )
    expect(query.accountId).toBe('acct-1')
    expect(query.mailboxId).toBe('mb-1')
    expect(query.smartMailboxId).toBeUndefined()
    expect(query.freeText).toBeNull()
    expect(query.sort).toEqual({ field: 'date', descending: true })
  })

  test('smart-mailbox view scopes by smart mailbox id', () => {
    const query = buildMailListQuery(
      { kind: 'smart-mailbox', id: 'smart-1', name: 'All Mail' },
      prepareServerSearchQuery(undefined),
      { columnId: 'from', direction: 'asc' },
    )
    expect(query.smartMailboxId).toBe('smart-1')
    expect(query.accountId).toBeUndefined()
    expect(query.sort).toEqual({ field: 'from', descending: false })
  })

  test('a prepared search rides as freeText', () => {
    const query = buildMailListQuery(
      { kind: 'smart-mailbox', id: 'smart-1', name: 'All Mail' },
      prepareServerSearchQuery('  hello   world '),
      { columnId: 'date', direction: 'desc' },
    )
    expect(query.freeText).toBe('hello world')
  })

  test('columns without server-side sorting fall back to date', () => {
    const query = buildMailListQuery(
      { kind: 'smart-mailbox', id: 'smart-1', name: 'All Mail' },
      prepareServerSearchQuery(undefined),
      { columnId: 'preview', direction: 'asc' },
    )
    expect(query.sort).toEqual({ field: 'date', descending: false })
  })
})
