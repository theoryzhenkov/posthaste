/**
 * MessageHeader — the Unsubscribe chip (List-Unsubscribe, RFC 2369/8058).
 *
 * The chip is registry-resolved like every other header action: it appears
 * ONLY when the detail carries parsed `listUnsubscribe` targets (never a
 * permanent icon), the one-click path parks behind the confirm dialog (never
 * runs bare from a click), and the mailto path routes to the host's composer
 * callback immediately (user-mediated — no dialog).
 */
import { describe, expect, it, mock } from 'bun:test'
import { fireEvent, render } from '@testing-library/react'

import type { ListUnsubscribe, MessageDetail } from '../src/api/types'
import type { EmailActions } from '../src/hooks/useEmailActions'
import { MessageHeader } from '../src/components/message-detail/MessageHeader'
import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

const ONE_CLICK: ListUnsubscribe = {
  https: 'https://news.example.test/unsub/opaque',
  mailto: 'mailto:unsub@example.test?subject=stop',
  oneClick: true,
}

function messageDetail(overrides: Partial<MessageDetail> = {}): MessageDetail {
  return {
    id: 'm-1',
    sourceId: 'acct-1',
    sourceName: 'Primary',
    sourceThreadId: 't-1',
    conversationId: 'c-1',
    subject: 'Weekly digest',
    fromName: 'Newsletter',
    fromEmail: 'news@example.test',
    to: [],
    preview: 'preview',
    receivedAt: '2026-05-31T00:00:00Z',
    hasAttachment: false,
    isRead: true,
    isFlagged: false,
    mailboxIds: ['inbox'],
    keywords: [],
    bodyHtml: null,
    bodyText: null,
    rawMessage: null,
    attachments: [],
    ...overrides,
  }
}

function makeSpyActions() {
  return {
    archive: mock(() => {}),
    trash: mock(() => {}),
    toggleFlag: mock(() => {}),
    snooze: mock(() => {}),
    unsubscribe: mock(() => Promise.resolve()),
    isPending: false,
  }
}

const noop = () => {}

function renderHeader(
  message: MessageDetail,
  over: Partial<{ onUnsubscribeMailto: (uri: string) => void }> = {},
) {
  const actions = makeSpyActions()
  const utils = render(
    <MessageHeader
      conversationSubject={message.subject}
      message={message}
      actions={actions as unknown as EmailActions}
      viewRole="inbox"
      onForward={noop}
      onReply={noop}
      onReplyAll={noop}
      onTag={noop}
      onUnsubscribeMailto={over.onUnsubscribeMailto}
      threadMessages={[message]}
    />,
  )
  return { ...utils, actions }
}

describe('MessageHeader unsubscribe chip', () => {
  it('renders the chip when the message carries unsubscribe data', () => {
    const { getByRole, unmount } = renderHeader(
      messageDetail({ listUnsubscribe: ONE_CLICK }),
    )
    expect(getByRole('button', { name: 'Unsubscribe' })).toBeDefined()
    unmount()
  })

  it('renders no chip without unsubscribe data (never a permanent icon)', () => {
    const { getByRole, queryByRole } = renderHeader(messageDetail())
    // The row itself rendered (other actions present) — only the chip is gone.
    expect(getByRole('button', { name: 'Archive' })).toBeDefined()
    expect(queryByRole('button', { name: 'Unsubscribe' })).toBeNull()
  })

  it('one-click NEVER runs bare — the click parks behind the confirm dialog', () => {
    const { getByRole, actions } = renderHeader(
      messageDetail({ listUnsubscribe: ONE_CLICK }),
    )
    fireEvent.click(getByRole('button', { name: 'Unsubscribe' }))
    // The runner is parked in the dialog host; the accept path is covered at
    // the resolver level (unsubscribeAction.test.ts), same as delete-permanently.
    expect(actions.unsubscribe).not.toHaveBeenCalled()
  })

  it('a mailto-only target routes straight to the composer callback', () => {
    const seen: string[] = []
    const { getByRole, actions } = renderHeader(
      messageDetail({
        listUnsubscribe: {
          mailto: 'mailto:unsub@example.test?subject=stop',
          oneClick: false,
        },
      }),
      { onUnsubscribeMailto: (uri) => seen.push(uri) },
    )
    fireEvent.click(getByRole('button', { name: 'Unsubscribe' }))
    expect(seen).toEqual(['mailto:unsub@example.test?subject=stop'])
    expect(actions.unsubscribe).not.toHaveBeenCalled()
  })
})
