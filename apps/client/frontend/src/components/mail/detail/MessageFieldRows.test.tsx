/**
 * Covers what the rows component itself decides. The reader's SELECTION comes
 * from a module-scoped store seeded once at import (and there is no DOM or
 * localStorage under `bun test`), so selection-dependent behaviour is tested
 * against `visibleDetailFields` in ../fields.test.ts instead — this file
 * exercises the default selection, which needs no seeding.
 */
import { describe, expect, test } from 'bun:test'
import { renderToStaticMarkup } from 'react-dom/server'

import type { MessageSummary } from '../../../data/transport/api/index'
import { MessageFieldRows } from './MessageFieldRows'

const message: MessageSummary = {
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

function render(overrides: Partial<MessageSummary> = {}, threadCount = 1) {
  return renderToStaticMarkup(
    <MessageFieldRows
      message={{ ...message, ...overrides }}
      threadMessageCount={threadCount}
    />,
  )
}

describe('rows', () => {
  test('always labels the arrival time', () => {
    expect(render()).toContain('Arrived at:')
  })

  test('shows the default selection, which is To and nothing else', () => {
    const html = render()
    expect(html).toContain('To:')
    expect(html).toContain('Grace')
    expect(html).not.toContain('CC:')
    expect(html).not.toContain('BCC:')
    expect(html).not.toContain('Reply-To:')
  })

  test('a populated but unselected CC stays hidden', () => {
    const html = render({ cc: [{ name: null, email: 'cc@example.com' }] })
    expect(html).not.toContain('CC:')
    expect(html).not.toContain('cc@example.com')
  })

  test('a To with no recipients renders no row', () => {
    expect(render({ to: [] })).not.toContain('To:')
  })

  test('names the message count only when the thread has more than one', () => {
    expect(render({}, 1)).not.toContain('messages')
    expect(render({}, 3)).toContain('3 messages')
  })

  test('the rows are the picker trigger, so selection is reachable', () => {
    // The menu's own items render only once it opens (Radix), which no DOM-less
    // render reaches; what is checkable here is that the rows carry the trigger
    // at all. The set of fields it offers is `fieldsForSurface('detail')`,
    // covered in ../fields.test.ts.
    expect(render()).toContain('data-slot="context-menu-trigger"')
  })
})
