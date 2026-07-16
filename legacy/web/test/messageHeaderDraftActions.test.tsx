/**
 * MessageHeader — the registry-driven action row (PLAN-L2 Slice 4).
 *
 * The header renders from `resolveActions(surface: 'detail-header')`, so this
 * covers both the D129 draft branch (edit + discard only, availability-driven
 * now) and the role-awareness fix: inside Trash the header offers Delete
 * permanently (confirm-gated) + Move to Inbox instead of Archive/Trash — the
 * latent bug the hand-rolled header had.
 */
import { describe, expect, it, mock } from 'bun:test'
import { fireEvent, render } from '@testing-library/react'

import {
  resolveActions,
  runResolvedWithConfirm,
  type ActionContext,
  type ActionServices,
} from '../src/actions'
import type { MessageDetail } from '../src/api/types'
import { SYSTEM_KEYWORDS } from '../src/domainVocabulary'
import type { EmailActions } from '../src/hooks/useEmailActions'
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

function makeSpyActions() {
  return {
    archive: mock(() => {}),
    trash: mock(() => {}),
    moveToInbox: mock(() => {}),
    deletePermanently: mock(() => {}),
    discardDraft: mock(() => {}),
    toggleFlag: mock(() => {}),
    snooze: mock(() => {}),
    isPending: false,
  }
}

const noop = () => {}

function renderHeader(
  message: MessageDetail,
  over: Partial<{
    viewRole: string | null
    actions: ReturnType<typeof makeSpyActions>
    onEditDraft: () => void
    onTag: () => void
    onOpenFocusedMessage: () => void
  }> = {},
) {
  const actions = over.actions ?? makeSpyActions()
  const utils = render(
    <MessageHeader
      conversationSubject={message.subject}
      message={message}
      actions={actions as unknown as EmailActions}
      viewRole={over.viewRole ?? 'inbox'}
      onForward={noop}
      onReply={noop}
      onReplyAll={noop}
      onTag={over.onTag ?? noop}
      onEditDraft={over.onEditDraft ?? noop}
      onOpenFocusedMessage={over.onOpenFocusedMessage ?? noop}
      threadMessages={[message]}
    />,
  )
  return { ...utils, actions }
}

describe('MessageHeader draft action row (D129, availability-driven)', () => {
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

  it('offers discard (not trash) on a draft, routing to the discard service', () => {
    const { getByRole, queryByRole, actions } = renderHeader(
      messageDetail({ keywords: [SYSTEM_KEYWORDS.Draft], draftId: 'd-1' }),
    )

    // No trash / archive / reply actions on a draft.
    expect(queryByRole('button', { name: 'Move to Trash' })).toBeNull()
    expect(queryByRole('button', { name: 'Archive' })).toBeNull()
    expect(queryByRole('button', { name: 'Reply' })).toBeNull()

    fireEvent.click(getByRole('button', { name: 'Discard draft' }))
    expect(actions.discardDraft).toHaveBeenCalledWith({
      sourceId: 'acct-1',
      messageId: 'm-1',
      draftId: 'd-1',
    })
  })

  it('keeps the standard action set for non-draft messages (no edit/discard)', () => {
    const { getByRole, queryByRole } = renderHeader(messageDetail())

    expect(queryByRole('button', { name: 'Edit draft' })).toBeNull()
    expect(queryByRole('button', { name: 'Discard draft' })).toBeNull()
    // The normal row still carries reply / archive / trash / snooze / flag /
    // tag / open — the pre-registry visible set.
    expect(getByRole('button', { name: 'Reply' })).toBeDefined()
    expect(getByRole('button', { name: 'Reply All' })).toBeDefined()
    expect(getByRole('button', { name: 'Forward' })).toBeDefined()
    expect(getByRole('button', { name: 'Archive' })).toBeDefined()
    expect(getByRole('button', { name: 'Move to Trash' })).toBeDefined()
    expect(getByRole('button', { name: 'Snooze' })).toBeDefined()
    expect(getByRole('button', { name: 'Flag' })).toBeDefined()
    expect(getByRole('button', { name: 'Tag' })).toBeDefined()
    expect(getByRole('button', { name: 'Open message' })).toBeDefined()
  })
})

describe('MessageHeader role-awareness (the Slice-4 fix)', () => {
  it('delegates archive/trash to the email services with the message ref', () => {
    const { getByRole, actions } = renderHeader(messageDetail())
    fireEvent.click(getByRole('button', { name: 'Archive' }))
    expect(actions.archive).toHaveBeenCalledWith({
      sourceId: 'acct-1',
      messageId: 'm-1',
    })
    fireEvent.click(getByRole('button', { name: 'Move to Trash' }))
    expect(actions.trash).toHaveBeenCalledWith({
      sourceId: 'acct-1',
      messageId: 'm-1',
    })
  })

  it('in TRASH offers delete-permanently (confirm-gated) + move-to-inbox, not archive/trash', () => {
    const { getByRole, queryByRole, actions } = renderHeader(messageDetail(), {
      viewRole: 'trash',
    })

    expect(queryByRole('button', { name: 'Archive' })).toBeNull()
    expect(queryByRole('button', { name: 'Move to Trash' })).toBeNull()
    expect(getByRole('button', { name: 'Move to Inbox' })).toBeDefined()

    // Delete permanently NEVER runs bare — clicking only parks the runner
    // behind the confirm dialog (the accept path is covered below at the
    // resolver level; the shared dialog host is covered by the keyboard tests).
    fireEvent.click(getByRole('button', { name: 'Delete permanently' }))
    expect(actions.deletePermanently).not.toHaveBeenCalled()
  })

  it('accepting the delete confirm runs the irreversible delete (header code path)', () => {
    // The exact ctx/services shape MessageHeader builds for the trash view,
    // driven through the same `runResolvedWithConfirm` gate its button uses.
    const email = makeSpyActions()
    const message = messageDetail()
    const services: ActionServices = {
      email: email as unknown as EmailActions,
      detail: { reply: noop, replyAll: noop, forward: noop },
    }
    const ctx: ActionContext = {
      targets: [
        {
          ref: { sourceId: message.sourceId, messageId: message.id },
          summary: message,
          isDraft: false,
          draftId: null,
          conversationId: message.conversationId,
        },
      ],
      viewRole: 'trash',
      activePane: 'list',
      surface: 'detail-header',
      inputOwner: 'mail',
      hasPendingMutation: false,
      connection: 'unknown',
    }
    const del = resolveActions(ctx, services).find(
      (r) => r.def.id === 'message.delete-permanently',
    )!
    let accept: (() => void) | null = null
    runResolvedWithConfirm(del, (_confirm, onConfirm) => {
      accept = onConfirm
    })
    expect(email.deletePermanently).not.toHaveBeenCalled()
    accept!()
    expect(email.deletePermanently).toHaveBeenCalledWith({
      sourceId: 'acct-1',
      messageId: 'm-1',
    })
  })

  it('in ARCHIVE hides archive and offers move-to-inbox', () => {
    const { getByRole, queryByRole, actions } = renderHeader(messageDetail(), {
      viewRole: 'archive',
    })
    expect(queryByRole('button', { name: 'Archive' })).toBeNull()
    fireEvent.click(getByRole('button', { name: 'Move to Inbox' }))
    expect(actions.moveToInbox).toHaveBeenCalledWith({
      sourceId: 'acct-1',
      messageId: 'm-1',
    })
  })

  it('renders the parameterized snooze as a popover trigger (the e2e anchor)', () => {
    const { getByRole } = renderHeader(messageDetail())
    const trigger = getByRole('button', { name: 'Snooze' })
    // The snooze e2e flow anchors on this exact selector.
    expect(trigger.getAttribute('data-slot')).toBe('popover-trigger')
    fireEvent.click(trigger)
    expect(trigger.getAttribute('data-state')).toBe('open')
  })

  it('a picked snooze preset runs email.snooze on the header target', () => {
    // The popover rows call `executeWith(option)` — drive it at the resolver
    // level with the header's exact ctx/services (portal content itself cannot
    // mount under the suite's shared Radix module state).
    const email = makeSpyActions()
    const message = messageDetail()
    const services: ActionServices = {
      email: email as unknown as EmailActions,
      detail: { reply: noop, replyAll: noop, forward: noop },
    }
    const ctx: ActionContext = {
      targets: [
        {
          ref: { sourceId: message.sourceId, messageId: message.id },
          summary: message,
          isDraft: false,
          draftId: null,
          conversationId: message.conversationId,
        },
      ],
      viewRole: 'inbox',
      activePane: 'list',
      surface: 'detail-header',
      inputOwner: 'mail',
      hasPendingMutation: false,
      connection: 'unknown',
    }
    const snooze = resolveActions(ctx, services).find(
      (r) => r.def.id === 'message.snooze',
    )!
    const tomorrow = snooze.params!.find((p) => p.label === 'Tomorrow')!
    void snooze.executeWith?.(tomorrow)
    expect(email.snooze).toHaveBeenCalledWith(
      { sourceId: 'acct-1', messageId: 'm-1' },
      Number(tomorrow.id),
    )
  })
})
