import { describe, expect, it } from 'bun:test'

import type { MessageSummary, SourceMessageRef } from '../src/api/types'
import {
  buildMessageContextActions,
  type ContextualAction,
} from '../src/actions/contextualActions'
import type { EmailActions } from '../src/hooks/useEmailActions'

const message: MessageSummary = {
  id: 'message-1',
  sourceId: 'account-1',
  sourceName: 'Account',
  sourceThreadId: 'thread-1',
  conversationId: 'conversation-1',
  subject: 'Hello',
  fromName: 'Sender',
  fromEmail: 'sender@example.test',
  to: [],
  preview: 'Preview',
  receivedAt: '2026-05-31T00:00:00Z',
  hasAttachment: false,
  isRead: false,
  isFlagged: false,
  mailboxIds: ['inbox'],
  keywords: [],
}

const target: SourceMessageRef = {
  sourceId: message.sourceId,
  messageId: message.id,
}

function makeActions(calls: string[]): EmailActions {
  return {
    toggleRead: () => calls.push('toggleRead'),
    markRead: () => calls.push('markRead'),
    toggleFlag: () => calls.push('toggleFlag'),
    setUserTags: () => calls.push('setUserTags'),
    archive: () => calls.push('archive'),
    trash: () => calls.push('trash'),
    moveToInbox: () => calls.push('moveToInbox'),
    deletePermanently: () => calls.push('deletePermanently'),
    clearError: () => calls.push('clearError'),
    errorMessage: null,
    isPending: false,
  }
}

function actionsForRole(viewRole: string | null): ContextualAction[] {
  const calls: string[] = []
  return buildMessageContextActions(
    makeActions(calls),
    { message, target, viewRole, surface: 'context-menu' },
    { onOpen: () => calls.push('open') },
  )
}

function actionIdsForRole(viewRole: string | null): string[] {
  return actionsForRole(viewRole).map((action) => action.id)
}

function runAction(actions: ContextualAction[], id: string) {
  const action = actions.find((candidate) => candidate.id === id)
  expect(action, id).toBeDefined()
  action?.run()
}

describe('contextual message actions', () => {
  it.each([
    [
      null,
      [
        'builtin.open',
        'builtin.toggle-read',
        'builtin.toggle-flag',
        'builtin.archive',
        'builtin.move-to-trash',
      ],
    ],
    [
      'inbox',
      [
        'builtin.open',
        'builtin.toggle-read',
        'builtin.toggle-flag',
        'builtin.archive',
        'builtin.move-to-trash',
      ],
    ],
    [
      'archive',
      [
        'builtin.open',
        'builtin.toggle-read',
        'builtin.toggle-flag',
        'builtin.move-to-inbox',
        'builtin.move-to-trash',
      ],
    ],
    [
      'junk',
      [
        'builtin.open',
        'builtin.toggle-read',
        'builtin.toggle-flag',
        'builtin.archive',
        'builtin.move-to-inbox',
        'builtin.move-to-trash',
      ],
    ],
    [
      'trash',
      [
        'builtin.open',
        'builtin.toggle-read',
        'builtin.toggle-flag',
        'builtin.move-to-inbox',
        'builtin.delete-permanently',
      ],
    ],
  ])('builds move actions for %p views', (viewRole, expectedIds) => {
    expect(actionIdsForRole(viewRole)).toEqual(expectedIds)
  })

  it('marks destructive move actions and labels trash restore actions clearly', () => {
    const actions = actionsForRole('trash')

    const restoreAction = actions.find(
      (action) => action.id === 'builtin.move-to-inbox',
    )
    const deleteAction = actions.find(
      (action) => action.id === 'builtin.delete-permanently',
    )

    expect(restoreAction?.title).toBe('Move to Inbox')
    expect(restoreAction?.destructive).toBeUndefined()
    expect(deleteAction?.title).toBe('Delete permanently')
    expect(deleteAction?.destructive).toBe(true)
  })

  it('wires action descriptors to the email action facade', () => {
    const calls: string[] = []
    const actions = buildMessageContextActions(
      makeActions(calls),
      { message, target, viewRole: 'trash', surface: 'context-menu' },
      { onOpen: () => calls.push('open') },
    )

    runAction(actions, 'builtin.open')
    runAction(actions, 'builtin.toggle-read')
    runAction(actions, 'builtin.toggle-flag')
    runAction(actions, 'builtin.move-to-inbox')
    runAction(actions, 'builtin.delete-permanently')

    expect(calls).toEqual([
      'open',
      'toggleRead',
      'toggleFlag',
      'moveToInbox',
      'deletePermanently',
    ])
  })
})
