import { describe, expect, test } from 'bun:test'

import type { MessageSummary } from '@/data/transport/api'

import {
  fieldsForSurface,
  getMessageField,
  hasMessageField,
  isMessageFieldId,
  messageFieldText,
  visibleDetailFields,
  type MessageFieldId,
} from './fields'

const base: MessageSummary = {
  id: 'msg-1',
  sourceId: 'acct-1',
  sourceName: 'Work',
  sourceThreadId: 'thread-1',
  conversationId: 'conv-1',
  subject: 'Quarterly report',
  fromName: 'Ada',
  fromEmail: 'ada@example.com',
  to: [{ name: 'Grace', email: 'grace@example.com' }],
  preview: 'Numbers attached',
  receivedAt: '2026-07-19T09:30:00.000Z',
  hasAttachment: false,
  isRead: true,
  isFlagged: false,
  mailboxIds: [],
  keywords: [],
}

describe('surfaces', () => {
  test('the list offers exactly the columns the table can lay out', () => {
    expect(fieldsForSurface('list')).toEqual([
      'unread',
      'flagged',
      'attachment',
      'subject',
      'from',
      'date',
      'source',
      'sourceMailbox',
      'tags',
      'preview',
    ])
  })

  test('the detail offers the reading fields, recipients included', () => {
    expect(fieldsForSurface('detail')).toEqual([
      'from',
      'source',
      'to',
      'cc',
      'bcc',
      'replyTo',
    ])
  })

  test('every field declares at least one surface', () => {
    const all = new Set([
      ...fieldsForSurface('list'),
      ...fieldsForSurface('detail'),
    ])
    for (const id of all) {
      expect(getMessageField(id).surfaces.length).toBeGreaterThan(0)
    }
  })

  test('every field has a non-empty label', () => {
    for (const id of fieldsForSurface('list')) {
      expect(getMessageField(id).label).not.toBe('')
    }
    for (const id of fieldsForSurface('detail')) {
      expect(getMessageField(id).label).not.toBe('')
    }
  })
})

describe('identity', () => {
  test('recognises its own ids and rejects anything else', () => {
    expect(isMessageFieldId('cc')).toBe(true)
    expect(isMessageFieldId('subject')).toBe(true)
    expect(isMessageFieldId('nope')).toBe(false)
    expect(isMessageFieldId(7)).toBe(false)
    expect(isMessageFieldId(undefined)).toBe(false)
  })

  test('an unknown id throws rather than yielding a blank field', () => {
    expect(() => getMessageField('nope' as MessageFieldId)).toThrow()
  })
})

describe('value derivation', () => {
  test('recipients read as names, falling back to the bare address', () => {
    const message: MessageSummary = {
      ...base,
      to: [
        { name: 'Grace', email: 'grace@example.com' },
        { name: null, email: 'anon@example.com' },
      ],
    }
    expect(messageFieldText('to', message)).toBe('Grace, anon@example.com')
  })

  test('from prefers the display name and falls back to the address', () => {
    expect(messageFieldText('from', base)).toBe('Ada')
    expect(messageFieldText('from', { ...base, fromName: null })).toBe(
      'ada@example.com',
    )
    expect(
      messageFieldText('from', { ...base, fromName: null, fromEmail: null }),
    ).toBe('')
  })

  test('tags exclude the provider keywords', () => {
    const message = { ...base, keywords: ['$seen', 'urgent', 'ops'] }
    expect(messageFieldText('tags', message)).toBe('urgent, ops')
  })

  test('a field with no textual form yields empty rather than throwing', () => {
    // `sourceMailbox` resolves against the mailbox directory, so only the list
    // can draw it; asking the registry for text must be safe and empty.
    expect(messageFieldText('sourceMailbox', base)).toBe('')
  })
})

describe('presence', () => {
  test('an absent optional recipient list is absent, not an empty row', () => {
    // The wire omits cc/bcc/replyTo entirely when empty, so the fields arrive
    // `undefined` — the case the detail pane must render as nothing at all.
    expect(base.cc).toBeUndefined()
    for (const id of ['cc', 'bcc', 'replyTo'] as const) {
      expect(messageFieldText(id, base)).toBe('')
      expect(hasMessageField(id, base)).toBe(false)
    }
  })

  test('an explicitly empty recipient list is also absent', () => {
    const message = { ...base, cc: [], bcc: [], replyTo: [] }
    expect(hasMessageField('cc', message)).toBe(false)
    expect(hasMessageField('bcc', message)).toBe(false)
  })

  test('a populated recipient list is present', () => {
    const message = {
      ...base,
      cc: [{ name: null, email: 'cc@example.com' }],
    }
    expect(hasMessageField('cc', message)).toBe(true)
    expect(messageFieldText('cc', message)).toBe('cc@example.com')
    // BCC stays absent: it is stripped in transit on received mail, so a
    // populated CC says nothing about it.
    expect(hasMessageField('bcc', message)).toBe(false)
  })

  test('boolean fields are present only when they are true', () => {
    expect(hasMessageField('unread', base)).toBe(false)
    expect(hasMessageField('unread', { ...base, isRead: false })).toBe(true)
    expect(hasMessageField('flagged', base)).toBe(false)
    expect(hasMessageField('flagged', { ...base, isFlagged: true })).toBe(true)
    expect(hasMessageField('attachment', base)).toBe(false)
    expect(
      hasMessageField('attachment', { ...base, hasAttachment: true }),
    ).toBe(true)
  })

  test('a missing subject or preview is absent', () => {
    const message = { ...base, subject: null, preview: null }
    expect(hasMessageField('subject', message)).toBe(false)
    expect(hasMessageField('preview', message)).toBe(false)
  })
})

describe('detail row selection', () => {
  const selectAll = ['to', 'cc', 'bcc', 'replyTo'] as const

  test('narrows the selection to the fields the message has', () => {
    // The BCC case is the one that matters: it is stripped in transit, so a
    // reader who enables it must not get an empty row on every message.
    expect(visibleDetailFields(selectAll, base)).toEqual(['to'])
  })

  test('includes a field once it is populated', () => {
    const message = {
      ...base,
      cc: [{ name: null, email: 'cc@example.com' }],
    }
    expect(visibleDetailFields(selectAll, message)).toEqual(['to', 'cc'])
  })

  test('orders by declaration, not by the order fields were enabled', () => {
    const message = {
      ...base,
      cc: [{ name: null, email: 'cc@example.com' }],
      replyTo: [{ name: null, email: 'r@example.com' }],
    }
    expect(visibleDetailFields(['replyTo', 'cc', 'to'], message)).toEqual([
      'to',
      'cc',
      'replyTo',
    ])
  })

  test('an empty selection yields no rows', () => {
    expect(visibleDetailFields([], base)).toEqual([])
  })

  test('a list-only field cannot leak into the detail rows', () => {
    // Guards the surface split: a stale stored id must not resurrect a column
    // as a detail row.
    expect(visibleDetailFields(['subject', 'preview', 'to'], base)).toEqual([
      'to',
    ])
  })
})
