import { describe, expect, it } from 'bun:test'

import type { MessageSummary } from '../src/api/types'
import {
  buildConversationTree,
  flatMessageRows,
} from '../src/components/message-list/conversationTree'

function message(
  id: string,
  conversationId: string,
  receivedAt: string,
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
  }
}

describe('conversation tree builder', () => {
  it('groups by conversation with root oldest-first and replies indented', () => {
    // Anchors arrive newest-first (date desc); a conversation appears at its
    // first-seen anchor but renders oldest-first internally.
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

    expect(rows.map((r) => [r.message.id, r.depth, r.isRoot])).toEqual([
      ['c1-early', 0, true],
      ['c1-late', 1, false],
      ['c2-only', 0, true],
    ])
    expect(rows[0].childCount).toBe(1)
    expect(visibleMessages.map((m) => m.id)).toEqual([
      'c1-early',
      'c1-late',
      'c2-only',
    ])
  })

  it('omits replies of collapsed conversations from the flattened rows', () => {
    const anchors = [message('c1-a', 'c1', '2026-05-01T00:00:00Z')]
    const complete = new Map<string, MessageSummary[]>([
      [
        'c1',
        [
          message('c1-a', 'c1', '2026-05-01T00:00:00Z'),
          message('c1-b', 'c1', '2026-05-02T00:00:00Z'),
        ],
      ],
    ])

    const { rows } = buildConversationTree({
      anchors,
      messagesByConversation: complete,
      collapsed: new Set(['c1']),
    })

    expect(rows).toHaveLength(1)
    expect(rows[0].message.id).toBe('c1-a')
    expect(rows[0].isRoot).toBe(true)
    expect(rows[0].collapsed).toBe(true)
    // childCount is reported even while collapsed (drives the chevron).
    expect(rows[0].childCount).toBe(1)
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
    expect(rows[0].childCount).toBe(1)
  })

  it('maps a flat message list to rows with no tree affordances', () => {
    const rows = flatMessageRows([
      message('m1', 'c1', '2026-05-01T00:00:00Z'),
      message('m2', 'c2', '2026-05-02T00:00:00Z'),
    ])
    expect(rows.map((r) => [r.message.id, r.depth, r.isRoot])).toEqual([
      ['m1', 0, false],
      ['m2', 0, false],
    ])
  })
})
