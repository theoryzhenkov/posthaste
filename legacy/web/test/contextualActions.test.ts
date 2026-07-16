/**
 * PLAN-L2 Slice 2 — the message context menu is resolver-driven.
 *
 * The `contextualActions.ts` shim is gone; `MessageRow` now builds an
 * `ActionContext` + `ActionServices` (with a row-scoped `row` bundle for the two
 * `open` entries) and calls `resolveActions` directly. This test pins the FULL
 * visible menu that construction produces — every item's id, label, icon,
 * destructive flag, section (which is what `MessageRow` draws separators from),
 * order, and role/draft gating — plus that each item delegates to the right
 * service. It reproduces exactly the inputs `MessageRow` assembles per row, so
 * it proves the menu renders the same items it did under the deleted shim.
 *
 * The canonical-id resolver invariants live in `actionRegistryParity.test.ts`
 * (state/move only); this file adds the two row-scoped `open` entries and the
 * whole-menu ordering the row renders.
 *
 * @spec docs/eph/PLAN-L2-action-registry.md
 * @spec docs/L1-ui#messagelist
 */
import { describe, expect, it } from 'bun:test'
import { MailOpen, MessagesSquare } from 'lucide-react'

// Barrel import registers the definitions, exactly as the app does at init.
import { resolveActions, type ResolvedAction } from '../src/actions'
import type {
  ActionContext,
  ActionServices,
  MessageTarget,
} from '../src/actions'
import type { MessageSummary, SourceMessageRef } from '../src/api/types'
import { SYSTEM_KEYWORDS } from '../src/domainVocabulary'
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
  draftId: null,
}

const target: SourceMessageRef = {
  sourceId: message.sourceId,
  messageId: message.id,
}

/** A spy EmailActions recording each method name it is asked to run. */
function makeActions(calls: string[]): EmailActions {
  return {
    toggleRead: () => calls.push('toggleRead'),
    markRead: () => calls.push('markRead'),
    toggleFlag: () => calls.push('toggleFlag'),
    setUserTags: () => calls.push('setUserTags'),
    archive: () => calls.push('archive'),
    trash: () => calls.push('trash'),
    discardDraft: () => calls.push('discardDraft'),
    moveToInbox: () => calls.push('moveToInbox'),
    deletePermanently: () => calls.push('deletePermanently'),
    clearError: () => calls.push('clearError'),
    errorMessage: null,
    isPending: false,
  } as unknown as EmailActions
}

/** Reproduce EXACTLY the ctx + services `MessageRow` builds for a row, then
 *  resolve the context-menu — the code path under test. */
function menuFor(
  msg: MessageSummary,
  viewRole: string | null,
  calls: string[],
): ResolvedAction[] {
  const services: ActionServices = {
    email: makeActions(calls),
    row: {
      open: () => calls.push('open'),
      viewConversation: () => calls.push('viewConversation'),
    },
  }
  const messageTarget: MessageTarget = {
    ref: target,
    summary: msg,
    isDraft: msg.keywords.includes(SYSTEM_KEYWORDS.Draft),
    draftId: msg.draftId,
    conversationId: msg.conversationId,
  }
  const ctx: ActionContext = {
    targets: [messageTarget],
    viewRole,
    activePane: 'list',
    surface: 'context-menu',
    inputOwner: 'mail',
    hasPendingMutation: false,
    connection: 'unknown',
  }
  return resolveActions(ctx, services)
}

function idsForRole(viewRole: string | null): string[] {
  return menuFor(message, viewRole, []).map((r) => r.def.id)
}

describe('message context menu (resolver-driven)', () => {
  it.each([
    [
      null,
      [
        'message.open',
        'message.view-conversation',
        'message.toggle-read',
        'message.toggle-flag',
        'message.archive',
        'message.move-to-trash',
      ],
    ],
    [
      'inbox',
      [
        'message.open',
        'message.view-conversation',
        'message.toggle-read',
        'message.toggle-flag',
        'message.archive',
        'message.move-to-trash',
      ],
    ],
    [
      'archive',
      [
        'message.open',
        'message.view-conversation',
        'message.toggle-read',
        'message.toggle-flag',
        'message.move-to-inbox',
        'message.move-to-trash',
      ],
    ],
    [
      'junk',
      [
        'message.open',
        'message.view-conversation',
        'message.toggle-read',
        'message.toggle-flag',
        'message.archive',
        'message.move-to-inbox',
        'message.move-to-trash',
      ],
    ],
    [
      'trash',
      [
        'message.open',
        'message.view-conversation',
        'message.toggle-read',
        'message.toggle-flag',
        'message.move-to-inbox',
        'message.delete-permanently',
      ],
    ],
  ])('renders the full ordered menu for %p views', (viewRole, expectedIds) => {
    expect(idsForRole(viewRole)).toEqual(expectedIds)
  })

  it('places the two open entries first, in their own section', () => {
    const menu = menuFor(message, 'inbox', [])
    expect(menu[0]?.def.id).toBe('message.open')
    expect(menu[0]?.title).toBe('Open')
    expect(menu[0]?.icon).toBe(MailOpen)
    expect(menu[1]?.def.id).toBe('message.view-conversation')
    expect(menu[1]?.title).toBe('View conversation')
    expect(menu[1]?.icon).toBe(MessagesSquare)
    // Both are section 'open'; the next item begins a new section (separator).
    expect(menu[0]?.def.section).toBe('open')
    expect(menu[1]?.def.section).toBe('open')
    expect(menu[2]?.def.section).not.toBe('open')
  })

  it('flips toggle labels and marks only destructive move actions', () => {
    const trash = menuFor(message, 'trash', [])
    const restore = trash.find((r) => r.def.id === 'message.move-to-inbox')
    const del = trash.find((r) => r.def.id === 'message.delete-permanently')
    expect(restore?.title).toBe('Move to Inbox')
    expect(restore?.def.destructive).toBeUndefined()
    expect(del?.title).toBe('Delete permanently')
    expect(del?.def.destructive).toBe(true)

    const read = menuFor({ ...message, isRead: true }, 'inbox', [])
    expect(read.find((r) => r.def.id === 'message.toggle-read')?.title).toBe(
      'Mark unread',
    )
    const flagged = menuFor({ ...message, isFlagged: true }, 'inbox', [])
    expect(flagged.find((r) => r.def.id === 'message.toggle-flag')?.title).toBe(
      'Unflag',
    )
  })

  it('wires each item to the matching service handler', () => {
    const calls: string[] = []
    const menu = menuFor(message, 'trash', calls)
    const run = (id: string) => {
      const action = menu.find((r) => r.def.id === id)
      expect(action, id).toBeDefined()
      action?.execute()
    }
    run('message.open')
    run('message.view-conversation')
    run('message.toggle-read')
    run('message.toggle-flag')
    run('message.move-to-inbox')
    run('message.delete-permanently')

    expect(calls).toEqual([
      'open',
      'viewConversation',
      'toggleRead',
      'toggleFlag',
      'moveToInbox',
      'deletePermanently',
    ])
  })

  it('routes a draft row to discard — never trash or delete (D127)', () => {
    const calls: string[] = []
    const draft: MessageSummary = { ...message, keywords: ['$draft'] }
    const menu = menuFor(draft, 'drafts', calls)
    const ids = menu.map((r) => r.def.id)

    expect(ids).toContain('message.discard-draft')
    expect(ids).not.toContain('message.move-to-trash')
    expect(ids).not.toContain('message.delete-permanently')

    const discard = menu.find((r) => r.def.id === 'message.discard-draft')
    expect(discard?.title).toBe('Discard draft')
    expect(discard?.def.destructive).toBe(true)

    discard?.execute()
    expect(calls).toEqual(['discardDraft'])
  })
})
