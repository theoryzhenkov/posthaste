/**
 * Contextual actions — the right-click context-menu builder.
 *
 * As of PLAN-L2 Slice 1 this is a THIN SHIM over the unified action registry +
 * {@link resolveActions}: the role-gated, pure message actions (toggle read/flag,
 * archive, move-to-inbox, and the trash/delete/discard trio) now live once in
 * `actions/defs/message.ts` and are resolved for the `'context-menu'` surface.
 * The builder still owns the two row-scoped `open` / `view-conversation` entries
 * (they migrate to definitions in Slice 2) and maps everything back to the
 * `ContextualAction` shape `MessageRow` already renders — so the menu renders
 * EXACTLY what it did before (same ids-as-keys, labels, icons, destructive
 * flags, group separators, and order). Zero behavior change.
 *
 * @spec docs/eph/PLAN-L2-action-registry.md
 * @spec docs/L1-ui#messagelist
 */
import type { LucideIcon } from 'lucide-react'
import { MailOpen, MessagesSquare } from 'lucide-react'
import type { MessageSummary, SourceMessageRef } from '../api/types'
import { SYSTEM_KEYWORDS } from '../domainVocabulary'
import type { EmailActions } from '../hooks/useEmailActions'
// Side-effect import: registers the message definitions into the registry.
import { messageActions } from './defs/message'
import { resolveActions } from './resolve'
import type { ActionContext, ActionSection, MessageTarget } from './types'

// Reference the registered set so the side-effect import is never elided and the
// registry is populated before the first resolve. (No runtime cost.)
void messageActions

/** Visual grouping; a separator is drawn between adjacent groups. */
export type ActionGroup = 'open' | 'state' | 'move'

export interface ContextualAction {
  /** Stable, namespaced id (used as the React key). */
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

/** Map a definition's richer {@link ActionSection} back to the three-value menu
 *  grouping the row renderer draws separators from. Every Slice-1 message
 *  definition is `state` or `move`. */
function sectionToGroup(section: ActionSection): ActionGroup {
  return section === 'state' ? 'state' : 'move'
}

/** The context menu's historical public ids are `builtin.*` and are used as
 *  React keys by `MessageRow`. The registry is canonical (`message.*`); this map
 *  preserves the legacy id the menu emitted so this slice is byte-for-byte
 *  behavior-neutral (Slice 2 collapses the shim and drops the map). */
const LEGACY_MENU_ID: Readonly<Record<string, string>> = {
  'message.toggle-read': 'builtin.toggle-read',
  'message.toggle-flag': 'builtin.toggle-flag',
  'message.archive': 'builtin.archive',
  'message.move-to-inbox': 'builtin.move-to-inbox',
  'message.move-to-trash': 'builtin.move-to-trash',
  'message.delete-permanently': 'builtin.delete-permanently',
  'message.discard-draft': 'builtin.discard-draft',
}

/**
 * Build the ordered, context-filtered actions for a message.
 *
 * The two `open` entries come from the row-scoped `hooks` (unchanged); the rest
 * are resolved from the registry for the `'context-menu'` surface. Availability
 * is derived from `viewRole` and draft-ness inside the definitions:
 * - Archive: any view that isn't already archive or trash.
 * - Move to Inbox: trash / archive / junk (restore).
 * - Move to Trash: any non-trash view, non-drafts.
 * - Delete permanently: trash only, non-drafts.
 * - Discard draft: drafts only.
 */
export function buildMessageContextActions(
  actions: EmailActions,
  ctx: MessageActionContext,
  hooks: { onOpen: () => void; onViewConversation: () => void },
): ContextualAction[] {
  const { message, target, viewRole } = ctx

  const messageTarget: MessageTarget = {
    ref: target,
    summary: message,
    isDraft: message.keywords.includes(SYSTEM_KEYWORDS.Draft),
    draftId: message.draftId,
    conversationId: message.conversationId,
  }
  const actionContext: ActionContext = {
    targets: [messageTarget],
    viewRole,
    activePane: 'list',
    surface: 'context-menu',
    inputOwner: 'mail',
    hasPendingMutation: actions.isPending,
    connection: 'unknown',
  }

  // Row-scoped entries stay owned by the builder until Slice 2 turns them into
  // definitions running through services.
  const open: ContextualAction[] = [
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
  ]

  const resolved = resolveActions(actionContext, { email: actions }).map(
    (r): ContextualAction => ({
      id: LEGACY_MENU_ID[r.def.id] ?? r.def.id,
      group: sectionToGroup(r.def.section),
      title: r.title,
      icon: r.icon,
      destructive: r.def.destructive,
      run: r.execute,
    }),
  )

  return [...open, ...resolved]
}
