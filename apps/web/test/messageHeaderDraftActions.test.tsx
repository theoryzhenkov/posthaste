import { describe, expect, it } from 'bun:test'
import { fireEvent, render } from '@testing-library/react'

import type { MessageDetail } from '../src/api/types'
import { SYSTEM_KEYWORDS } from '../src/domainVocabulary'
import { MessageHeader } from '../src/components/message-detail/MessageHeader'
import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

function messageDetail(overrides: Partial<MessageDetail> = {}): MessageDetail {
  return {
    id: 'm-1',
    sourceId: 'acct-1',
    sourceName: 'Primary',
    sourceThreadId: 't-1',
    conversationId: 'c-1',
    subject: 'Subject',
    fromName: 'Sender',
    fromEmail: 'sender@example.com',
    to: [],
    preview: 'preview',
    receivedAt: '2026-05-31T00:00:00Z',
    hasAttachment: false,
    isRead: true,
    isFlagged: false,
    mailboxIds: ['drafts'],
    keywords: [],
    bodyHtml: null,
    bodyText: null,
    rawMessage: null,
    attachments: [],
    ...overrides,
  }
}

const noop = () => {}

function renderHeader(
  message: MessageDetail,
  handlers: Partial<{
    onEditDraft: () => void
    onDiscardDraft: () => void
    onTrash: () => void
  }> = {},
) {
  return render(
    <MessageHeader
      conversationSubject={message.subject}
      message={message}
      onArchive={noop}
      onSnooze={noop}
      onForward={noop}
      onReply={noop}
      onReplyAll={noop}
      onToggleFlag={noop}
      onTag={noop}
      onTrash={handlers.onTrash ?? noop}
      onEditDraft={handlers.onEditDraft ?? noop}
      onDiscardDraft={handlers.onDiscardDraft ?? noop}
      threadMessages={[message]}
    />,
  )
}

describe('MessageHeader draft action row (D129)', () => {
  it('shows the edit-draft icon only for drafts and opens the compose flow', () => {
    let editOpened = 0
    const { getByRole } = renderHeader(
      messageDetail({ keywords: [SYSTEM_KEYWORDS.Draft] }),
      { onEditDraft: () => (editOpened += 1) },
    )

    const editButton = getByRole('button', { name: 'Edit draft' })
    // It is an icon action, not the old ad-hoc text button — no visible label.
    expect(editButton.textContent?.trim()).toBe('')

    fireEvent.click(editButton)
    expect(editOpened).toBe(1)
  })

  it('offers discard (not trash) on a draft, routing to the discard handler', () => {
    let discarded = 0
    const { getByRole, queryByRole } = renderHeader(
      messageDetail({ keywords: [SYSTEM_KEYWORDS.Draft] }),
      { onDiscardDraft: () => (discarded += 1) },
    )

    // No trash action on a draft.
    expect(queryByRole('button', { name: 'Trash' })).toBeNull()

    const discardButton = getByRole('button', { name: 'Discard draft' })
    fireEvent.click(discardButton)
    expect(discarded).toBe(1)
  })

  it('keeps the standard action set for non-draft messages (no edit/discard)', () => {
    const { getByRole, queryByRole } = renderHeader(
      messageDetail({ keywords: [] }),
    )

    expect(queryByRole('button', { name: 'Edit draft' })).toBeNull()
    expect(queryByRole('button', { name: 'Discard draft' })).toBeNull()
    // The normal row still carries reply / archive / trash.
    expect(getByRole('button', { name: 'Reply' })).toBeDefined()
    expect(getByRole('button', { name: 'Archive' })).toBeDefined()
    expect(getByRole('button', { name: 'Trash' })).toBeDefined()
  })
})
