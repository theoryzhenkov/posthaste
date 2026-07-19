import { describe, expect, test } from 'bun:test'
import { renderToStaticMarkup } from 'react-dom/server'

import type { MessageSummary } from '../../../data/transport/api/index'
import { buildThreadListLayout } from '../thread/columns'
import { MessageRow } from './MessageRow'

const message: MessageSummary = {
  id: 'msg-1',
  sourceId: 'acct-1',
  sourceName: 'Work',
  sourceThreadId: 'thread-1',
  conversationId: 'conv-1',
  subject: 'Quarterly report',
  fromName: 'Ada',
  fromEmail: 'ada@example.com',
  to: [],
  preview: 'Numbers attached',
  receivedAt: '2026-07-19T09:30:00.000Z',
  hasAttachment: false,
  isRead: true,
  isFlagged: false,
  mailboxIds: [],
  keywords: [],
}

function renderRow(state: {
  isSelected: boolean
  isActive: boolean
  isPaneActive?: boolean
}): string {
  return renderToStaticMarkup(
    <MessageRow
      message={message}
      isSelected={state.isSelected}
      isActive={state.isActive}
      isPaneActive={state.isPaneActive ?? true}
      isStriped={false}
      columns={['subject']}
      layout={buildThreadListLayout(['subject'])}
      contextMenuFor={() => []}
      onSelectMessage={() => {}}
      onViewConversation={() => {}}
      mailboxDirectory={{ resolve: () => null, list: () => [] }}
      excludeMailboxId={null}
    />,
  )
}

// The three row states of the cursor/active split: the ACTIVE (opened) row
// carries the strong selection accent, a SELECTED-but-not-active row (the
// diverged `j`/`k` cursor) reuses the sidebar's muted selection treatment,
// and a plain row shows neither.
describe('MessageRow highlight states', () => {
  test('active row gets the strong selection highlight', () => {
    const markup = renderRow({ isSelected: true, isActive: true })
    expect(markup).toContain('bg-[var(--list-selection)]')
    expect(markup).not.toContain('bg-[var(--list-selection-muted)]')
  })

  test('selected-but-not-active row gets the muted (sidebar) treatment', () => {
    const markup = renderRow({ isSelected: true, isActive: false })
    expect(markup).toContain('bg-[var(--list-selection-muted)]')
    expect(markup).not.toContain('bg-[var(--list-selection)]')
  })

  test('a row that is neither shows the plain zebra background', () => {
    const markup = renderRow({ isSelected: false, isActive: false })
    expect(markup).toContain('bg-[var(--list-zebra)]')
    expect(markup).not.toContain('list-selection')
  })

  test('the active row greys out while the list pane is not focused', () => {
    const markup = renderRow({
      isSelected: true,
      isActive: true,
      isPaneActive: false,
    })
    expect(markup).toContain('bg-[var(--list-selection-muted)]')
  })
})
