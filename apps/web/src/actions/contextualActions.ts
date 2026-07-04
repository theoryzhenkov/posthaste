/**
 * Contextual actions — the first slice of a general registry for context-aware,
 * eventually user-defined actions on items (see
 * `docs/eph/PLAN-L1-contextual-actions.md`).
 *
 * Phase 0 covers built-in message actions surfaced in the right-click menu, with
 * availability derived from the current view's mailbox role: e.g. in Trash the
 * menu offers "Move to Inbox" + "Delete permanently" instead of a no-op "Move to
 * Trash". The builder is pure (icons are component references, not JSX) so it
 * can be unit-tested and reused across surfaces (palette, keyboard) later.
 *
 * @spec docs/L1-ui#messagelist
 */
import type { LucideIcon } from 'lucide-react'
import {
  Archive,
  Eye,
  EyeOff,
  Inbox,
  MailOpen,
  MessagesSquare,
  Star,
  Trash2,
} from 'lucide-react'
import type { MessageSummary, SourceMessageRef } from '../api/types'
import { SYSTEM_KEYWORDS } from '../domainVocabulary'
import type { EmailActions } from '../hooks/useEmailActions'

/** Visual grouping; a separator is drawn between adjacent groups. */
export type ActionGroup = 'open' | 'state' | 'move'

export interface ContextualAction {
  /** Stable, namespaced id (e.g. `builtin.move-to-inbox`). */
  id: string
  group: ActionGroup
  title: string
  icon: LucideIcon
  destructive?: boolean
  run: () => void
}

/** Resolved context for a single message target invoked from a surface. */
export interface MessageActionContext {
  message: MessageSummary
  target: SourceMessageRef
  /**
   * Mailbox role of the current view (a JMAP role string; known values match
   * `KnownMailboxRole`), or null when ambiguous (a search view, or a smart
   * mailbox with no assigned role). For source-mailbox views it is the
   * mailbox's role; for role-tagged smart mailboxes it is the smart mailbox's
   * assigned role, so e.g. the Trash smart mailbox surfaces Delete permanently.
   */
  viewRole: string | null
  surface: 'context-menu'
}

/** Roles from which a message is "removed" and can be restored to the inbox. */
function isRestorableRole(role: string | null): boolean {
  return role === 'trash' || role === 'archive' || role === 'junk'
}

/**
 * Build the ordered, context-filtered actions for a message. Availability is
 * derived from `viewRole`:
 * - Archive: any view that isn't already archive or trash.
 * - Move to Inbox: trash / archive / junk (restore).
 * - Move to Trash: any view that isn't already trash.
 * - Delete permanently: trash only.
 */
export function buildMessageContextActions(
  actions: EmailActions,
  ctx: MessageActionContext,
  hooks: { onOpen: () => void; onViewConversation: () => void },
): ContextualAction[] {
  const { message, target, viewRole } = ctx
  const list: ContextualAction[] = [
    {
      id: 'builtin.open',
      group: 'open',
      title: 'Open',
      icon: MailOpen,
      run: hooks.onOpen,
    },
    {
      id: 'builtin.view-conversation',
      group: 'open',
      title: 'View conversation',
      icon: MessagesSquare,
      run: hooks.onViewConversation,
    },
    {
      id: 'builtin.toggle-read',
      group: 'state',
      title: message.isRead ? 'Mark unread' : 'Mark read',
      icon: message.isRead ? EyeOff : Eye,
      run: () => actions.toggleRead(message),
    },
    {
      id: 'builtin.toggle-flag',
      group: 'state',
      title: message.isFlagged ? 'Unflag' : 'Flag',
      icon: Star,
      run: () => actions.toggleFlag(message),
    },
  ]

  if (viewRole !== 'archive' && viewRole !== 'trash') {
    list.push({
      id: 'builtin.archive',
      group: 'move',
      title: 'Archive',
      icon: Archive,
      run: () => actions.archive(target),
    })
  }

  if (isRestorableRole(viewRole)) {
    list.push({
      id: 'builtin.move-to-inbox',
      group: 'move',
      title: 'Move to Inbox',
      icon: Inbox,
      run: () => actions.moveToInbox(target),
    })
  }

  if (message.keywords.includes(SYSTEM_KEYWORDS.Draft)) {
    // D127: a draft is discarded (hard delete via the draft-delete op), never
    // trashed. The trash / delete-permanently actions are not offered on drafts.
    list.push({
      id: 'builtin.discard-draft',
      group: 'move',
      title: 'Discard draft',
      icon: Trash2,
      destructive: true,
      run: () => actions.discardDraft(target),
    })
  } else if (viewRole !== 'trash') {
    list.push({
      id: 'builtin.move-to-trash',
      group: 'move',
      title: 'Move to Trash',
      icon: Trash2,
      destructive: true,
      run: () => actions.trash(target),
    })
  } else {
    list.push({
      id: 'builtin.delete-permanently',
      group: 'move',
      title: 'Delete permanently',
      icon: Trash2,
      destructive: true,
      run: () => actions.deletePermanently(target),
    })
  }

  return list
}
