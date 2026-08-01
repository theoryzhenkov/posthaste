/**
 * Covers what the rows component itself decides. The reader's SELECTION comes
 * from a module-scoped store seeded once at import (and there is no DOM or
 * localStorage under `bun test`), so selection-dependent behaviour is tested
 * against `visibleDetailFields` in ../fields.test.ts instead — this file
 * exercises the DEFAULT selection, which needs no seeding and which is now
 * load-bearing: these rows are the whole message header, so a default that
 * dropped a field would delete it from the reading pane outright.
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

function render(
  overrides: Partial<MessageSummary> = {},
  props: { conversationSubject?: string | null; threadCount?: number } = {},
) {
  return renderToStaticMarkup(
    <MessageFieldRows
      conversationSubject={props.conversationSubject}
      message={{ ...message, ...overrides }}
      threadMessageCount={props.threadCount ?? 1}
    />,
  )
}

describe('rows', () => {
  test('always labels the arrival time', () => {
    expect(render()).toContain('Arrived at:')
  })

  test('the default selection names the message and its sender', () => {
    // The header draws nothing of its own any more, so these defaults are
    // what stands between a reader and a message with no subject and no
    // sender on it.
    const html = render()
    expect(html).toContain('Quarterly report')
    expect(html).toContain('From:')
    expect(html).toContain('Ada')
    expect(html).toContain('To:')
    expect(html).toContain('Grace')
  })

  test('the subject reads as a heading and prints no key', () => {
    // Its declared presentation: prominence is a property of the field, so
    // the subject can be as toggleable as Reply-To and still be the thing the
    // eye lands on. A `Subject:` in front of it would say less than it does.
    const html = render()
    expect(html).toContain('text-heading')
    expect(html).not.toContain('Subject:')
  })

  test('a label-less row spans both columns rather than indenting', () => {
    expect(render()).toContain('col-span-2')
  })

  test('Reply-To is the one detail field left off by default', () => {
    const html = render({ replyTo: [{ name: null, email: 'r@example.com' }] })
    expect(html).not.toContain('Reply-To:')
    expect(html).not.toContain('r@example.com')
  })

  test('CC and BCC are on by default, and cost nothing when absent', () => {
    expect(render()).not.toContain('CC:')
    const html = render({ cc: [{ name: null, email: 'cc@example.com' }] })
    expect(html).toContain('CC:')
    expect(html).toContain('cc@example.com')
  })

  test('the conversation subject wins over the message', () => {
    expect(render({}, { conversationSubject: 'Thread subject' })).toContain(
      'Thread subject',
    )
  })

  test('a subject-less message says so rather than losing the row', () => {
    expect(render({ subject: null })).toContain('(no subject)')
  })

  test('the sender keeps its click-to-search buttons, name and address', () => {
    const html = render()
    expect(html).toContain('Search emails from this sender')
    expect(html).toContain('&lt;ada@example.com&gt;')
  })

  test('tags render as chips, and no tags renders no row at all', () => {
    expect(render()).not.toContain('Tags:')
    const html = render({ keywords: ['$seen', 'invoice'] })
    expect(html).toContain('Tags:')
    expect(html).toContain('invoice')
    // System keywords are message state, shown elsewhere; only user tags here.
    expect(html).not.toContain('$seen')
  })

  test('a To with no recipients renders no row', () => {
    expect(render({ to: [] })).not.toContain('To:')
  })

  test('names the message count only when the thread has more than one', () => {
    expect(render({}, { threadCount: 1 })).not.toContain('messages')
    expect(render({}, { threadCount: 3 })).toContain('3 messages')
  })

  test('the picker is reachable by right-click AND by a visible button', () => {
    // Menu and popover items render only once open (Radix portals), which no
    // DOM-less render reaches; what is checkable here is that both ways in
    // exist. The set of fields they offer is `fieldPickerOptions`, covered in
    // ../thread/fieldPicker.test.tsx.
    const html = render()
    expect(html).toContain('data-slot="context-menu-trigger"')
    expect(html).toContain('aria-label="Choose header rows"')
  })
})
