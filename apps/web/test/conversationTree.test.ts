import { describe, expect, it } from 'bun:test'

import type { MessageSummary } from '../src/api/types'
import {
  buildConversationTree,
  flatMessageRows,
} from '../src/components/message-list/conversationTree'
import { messageKey } from '../src/components/message-list/model'

function message(
  id: string,
  conversationId: string,
  receivedAt: string,
  threading: { rfcMessageId?: string; inReplyTo?: string } = {},
): MessageSummary {
  return {
    id,
    sourceId: 'account-1',
    sourceName: 'Account',
    sourceThreadId: `thread-${conversationId}`,
    conversationId,
    subject: 'Subject',
    fromName: 'Sender',
    fromEmail: 'sender@example.test',
    to: [],
    preview: 'Preview',
    receivedAt,
    hasAttachment: false,
    isRead: false,
    isFlagged: false,
    mailboxIds: ['inbox'],
    keywords: [],
    rfcMessageId: threading.rfcMessageId ?? null,
    inReplyTo: threading.inReplyTo ?? null,
  }
}

describe('conversation tree builder', () => {
  it('groups by conversation, root oldest-first, unthreaded replies under root', () => {
    // Anchors arrive newest-first (date desc); a conversation appears at its
    // first-seen anchor but renders oldest-first internally. With no threading
    // headers, every reply hangs directly under the root (depth 1).
    const anchors = [
      message('c1-late', 'c1', '2026-05-03T00:00:00Z'),
      message('c2-only', 'c2', '2026-05-02T00:00:00Z'),
    ]
    const complete = new Map<string, MessageSummary[]>([
      [
        'c1',
        [
          message('c1-late', 'c1', '2026-05-03T00:00:00Z'),
          message('c1-early', 'c1', '2026-05-01T00:00:00Z'),
        ],
      ],
    ])

    const { rows, visibleMessages } = buildConversationTree({
      anchors,
      messagesByConversation: complete,
      collapsed: new Set(),
    })

    expect(rows.map((r) => [r.message.id, r.depth, r.hasChildren])).toEqual([
      ['c1-early', 0, true],
      ['c1-late', 1, false],
      ['c2-only', 0, false],
    ])
    expect(visibleMessages.map((m) => m.id)).toEqual([
      'c1-early',
      'c1-late',
      'c2-only',
    ])
  })

  it('builds real depth from In-Reply-To chains', () => {
    // root <- reply1 <- reply2 (a deep chain), plus reply1b also under root.
    const complete = new Map<string, MessageSummary[]>([
      [
        'c1',
        [
          message('root', 'c1', '2026-05-01T00:00:00Z', {
            rfcMessageId: '<a>',
          }),
          message('reply1', 'c1', '2026-05-02T00:00:00Z', {
            rfcMessageId: '<b>',
            inReplyTo: '<a>',
          }),
          message('reply2', 'c1', '2026-05-03T00:00:00Z', {
            rfcMessageId: '<c>',
            inReplyTo: '<b>',
          }),
          message('reply1b', 'c1', '2026-05-04T00:00:00Z', {
            rfcMessageId: '<d>',
            inReplyTo: '<a>',
          }),
        ],
      ],
    ])
    const { rows } = buildConversationTree({
      anchors: [message('root', 'c1', '2026-05-01T00:00:00Z')],
      messagesByConversation: complete,
      collapsed: new Set(),
    })
    // Preorder: root, reply1, reply1's child reply2, then reply1b under root.
    expect(rows.map((r) => [r.message.id, r.depth])).toEqual([
      ['root', 0],
      ['reply1', 1],
      ['reply2', 2],
      ['reply1b', 1],
    ])
    // Both root and reply1 have children; the leaves do not.
    expect(rows.map((r) => r.hasChildren)).toEqual([true, true, false, false])
  })

  it('orphan replies (parent not in thread) fall back to the root', () => {
    const complete = new Map<string, MessageSummary[]>([
      [
        'c1',
        [
          message('root', 'c1', '2026-05-01T00:00:00Z', {
            rfcMessageId: '<a>',
          }),
          message('orphan', 'c1', '2026-05-02T00:00:00Z', {
            rfcMessageId: '<x>',
            inReplyTo: '<missing>',
          }),
        ],
      ],
    ])
    const { rows } = buildConversationTree({
      anchors: [message('root', 'c1', '2026-05-01T00:00:00Z')],
      messagesByConversation: complete,
      collapsed: new Set(),
    })
    expect(rows.map((r) => [r.message.id, r.depth])).toEqual([
      ['root', 0],
      ['orphan', 1],
    ])
  })

  it('collapsing a node omits its subtree, keyed by message key', () => {
    const root = message('c1-a', 'c1', '2026-05-01T00:00:00Z', {
      rfcMessageId: '<a>',
    })
    const complete = new Map<string, MessageSummary[]>([
      [
        'c1',
        [
          root,
          message('c1-b', 'c1', '2026-05-02T00:00:00Z', {
            rfcMessageId: '<b>',
            inReplyTo: '<a>',
          }),
        ],
      ],
    ])

    const { rows } = buildConversationTree({
      anchors: [root],
      messagesByConversation: complete,
      collapsed: new Set([messageKey(root)]),
    })

    expect(rows).toHaveLength(1)
    expect(rows[0].message.id).toBe('c1-a')
    expect(rows[0].collapsed).toBe(true)
    // hasChildren stays true while collapsed (drives the chevron).
    expect(rows[0].hasChildren).toBe(true)
  })

  it('falls back to anchor messages before the full conversation is fetched', () => {
    const anchors = [
      message('c1-a', 'c1', '2026-05-01T00:00:00Z'),
      message('c1-b', 'c1', '2026-05-02T00:00:00Z'),
    ]

    const { rows } = buildConversationTree({
      anchors,
      messagesByConversation: new Map(),
      collapsed: new Set(),
    })

    expect(rows.map((r) => r.message.id)).toEqual(['c1-a', 'c1-b'])
    expect(rows[0].hasChildren).toBe(true)
  })

  it('maps a flat message list to rows with no tree affordances', () => {
    const rows = flatMessageRows([
      message('m1', 'c1', '2026-05-01T00:00:00Z'),
      message('m2', 'c2', '2026-05-02T00:00:00Z'),
    ])
    expect(rows.map((r) => [r.message.id, r.depth, r.hasChildren])).toEqual([
      ['m1', 0, false],
      ['m2', 0, false],
    ])
  })
})
