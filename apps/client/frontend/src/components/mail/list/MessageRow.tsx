/**
 * Single message row in the message list.
 *
 * Renders sender, subject, preview, relative timestamp, unread dot,
 * flag star, attachment state, and source tag.
 *
 */
import { Fragment, memo, useCallback } from 'react'
import { ChevronDown, ChevronRight } from 'lucide-react'
import {
  resolveActions,
  type ActionContext,
  type ActionServices,
} from '../../../commands/index'
import type { MessageSummary } from '../../../data/transport/api/index'
import { SYSTEM_KEYWORDS } from '../../../domain/vocabulary'
import type { EmailActions } from '../../../data/hooks/useEmailActions'
import { cn } from '../../../lib/cn'
import type { ConversationTreeRow } from './model/conversationTree'
import { messageKey } from './model/model'
import type { MailboxDirectory } from './model/useMailboxDirectory'
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuSub,
  ContextMenuSubContent,
  ContextMenuSubTrigger,
  ContextMenuTrigger,
} from '../../ui/overlay/context-menu'
import {
  type ColumnId,
  type ColumnRenderContext,
  type ThreadListLayout,
  getColumnDef,
} from '../thread/columns'

interface MessageRowProps {
  message: MessageSummary
  isSelected: boolean
  isPaneActive?: boolean
  isStriped: boolean
  onSelectMessage: (message: MessageSummary) => void
  columns: ColumnId[]
  layout: ThreadListLayout
  actions: EmailActions
  /** Role of the current view, used to derive contextual actions; null = ambiguous. */
  viewRole: string | null
  /** Filter the view to this message's conversation (contextual action). */
  onViewConversation: (message: MessageSummary) => void
  /** Tree placement in conversation view; undefined in the flat list. */
  treeRow?: ConversationTreeRow
  /** Toggle one node's collapse state, keyed by message key (conversation view
   *  only). */
  onToggleCollapse?: (messageKey: string) => void
  /** Cache-only mailbox resolver, consumed by the `sourceMailbox` column cell. */
  mailboxDirectory: MailboxDirectory
  /** The mailbox already being viewed (single source-mailbox views), excluded
   *  from the `sourceMailbox` cell's candidate memberships when possible. */
  excludeMailboxId: string | null
}

/** Left offset applied per reply depth. The root (depth 0) gets none, so it sits
 *  flush with the flat list; each reply level indents further. */
const TREE_INDENT_PX = 22

/**
 * Fixed-height message row displaying sender, subject,
 * preview, date, unread state, flag, and source.
 */
export const MessageRow = memo(function MessageRow({
  message,
  isSelected,
  isPaneActive = false,
  isStriped,
  onSelectMessage,
  columns,
  layout,
  actions,
  viewRole,
  onViewConversation,
  treeRow,
  onToggleCollapse,
  mailboxDirectory,
  excludeMailboxId,
}: MessageRowProps) {
  const messageRef = { messageId: message.id, sourceId: message.sourceId }
  const renderContext: ColumnRenderContext = {
    mailboxDirectory,
    excludeMailboxId,
  }
  const handleSelect = useCallback(() => {
    onSelectMessage(message)
  }, [message, onSelectMessage])
  // The context menu resolves directly from the registry for this row's target.
  // `services.row` binds the two `open` entries to the row callbacks (the same
  // wiring the deleted shim owned); `email` carries the domain mutations. Built
  // per render — cheap plain objects, no hooks inside.
  const services: ActionServices = {
    email: actions,
    row: { open: onSelectMessage, viewConversation: onViewConversation },
    // The account's mailbox read model (already subscribed once per account by
    // useMailboxDirectory) — options source for the parameterized "Move to ▸".
    mailboxes: { list: mailboxDirectory.list },
  }
  const actionContext: ActionContext = {
    targets: [
      {
        ref: messageRef,
        summary: message,
        isDraft: message.keywords.includes(SYSTEM_KEYWORDS.Draft),
        draftId: message.draftId,
        conversationId: message.conversationId,
      },
    ],
    viewRole,
    activePane: 'list',
    surface: 'context-menu',
    inputOwner: 'mail',
    hasPendingMutation: actions.isPending,
    connection: 'unknown',
  }
  const contextActions = resolveActions(actionContext, services)
  const row = (
    <button
      className={cn(
        'flex h-full w-full items-center gap-0',
        'text-left text-[13px] transition-colors',
        'ph-focus-ring',
        isSelected &&
          isPaneActive &&
          'bg-[var(--list-selection)] text-[var(--list-selection-foreground)]',
        isSelected &&
          !isPaneActive &&
          'bg-[var(--list-selection-muted)] text-[var(--list-selection-muted-foreground)]',
        !isSelected &&
          (isStriped
            ? 'bg-[var(--list-zebra-alt)] text-panel-foreground hover:bg-[var(--list-hover)]'
            : 'bg-[var(--list-zebra)] text-panel-foreground hover:bg-[var(--list-hover)]'),
      )}
      onClick={handleSelect}
      onContextMenu={handleSelect}
      type="button"
    >
      {treeRow && (
        <TreeGutter treeRow={treeRow} onToggleCollapse={onToggleCollapse} />
      )}
      <div
        className="grid h-full min-w-0 flex-1 items-center gap-0"
        style={layout.gridStyle}
      >
        {columns.map((columnId) => {
          const def = getColumnDef(columnId)
          return (
            <div
              key={columnId}
              className={cn(
                'flex h-full min-w-0 items-center gap-2 overflow-hidden px-2.5 pr-4',
                columnId === 'subject' && 'pl-3',
                def.align === 'right' && 'justify-end text-right',
                def.align === 'center' && 'justify-center px-0',
              )}
            >
              {def.render(message, renderContext)}
            </div>
          )
        })}
      </div>
    </button>
  )

  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>{row}</ContextMenuTrigger>
      <ContextMenuContent className="min-w-44">
        {contextActions.map((action, index) => {
          const previous = contextActions[index - 1]
          const Icon = action.icon
          return (
            <Fragment key={action.def.id}>
              {previous && previous.def.section !== action.def.section && (
                <ContextMenuSeparator />
              )}
              {action.params ? (
                // A PARAMETERIZED action renders as a submenu ("Move to ▸"):
                // one row per resolved option, each running `executeWith`.
                <ContextMenuSub>
                  <ContextMenuSubTrigger>
                    <Icon size={14} />
                    {/* The chevron already signals "more"; drop a trailing ellipsis. */}
                    {action.title.replace(/…$/, '')}
                  </ContextMenuSubTrigger>
                  <ContextMenuSubContent className="min-w-40">
                    {action.params.map((option) => (
                      <ContextMenuItem
                        key={option.id}
                        onSelect={() => void action.executeWith?.(option)}
                      >
                        {option.label}
                      </ContextMenuItem>
                    ))}
                  </ContextMenuSubContent>
                </ContextMenuSub>
              ) : (
                <ContextMenuItem
                  variant={action.def.destructive ? 'destructive' : 'default'}
                  onSelect={action.execute}
                >
                  <Icon size={14} />
                  {action.title}
                </ContextMenuItem>
              )}
            </Fragment>
          )
        })}
      </ContextMenuContent>
    </ContextMenu>
  )
})

/**
 * Leading gutter for conversation view: an indent that grows with reply depth
 * (the root has none, so it stays flush with the flat list) plus a collapse
 * chevron on any node that has replies. The chevron is absolutely positioned so
 * it sits just left of the content without adding to the indent — the root's
 * chevron hangs over its lead padding rather than offsetting it.
 */
function TreeGutter({
  treeRow,
  onToggleCollapse,
}: {
  treeRow: ConversationTreeRow
  onToggleCollapse?: (messageKey: string) => void
}) {
  const indent = treeRow.depth * TREE_INDENT_PX
  const Chevron = treeRow.collapsed ? ChevronRight : ChevronDown
  const isRoot = treeRow.depth === 0
  return (
    <span
      aria-hidden={!treeRow.hasChildren}
      className="relative flex h-full shrink-0 items-center overflow-visible"
      style={{ width: indent }}
    >
      {treeRow.hasChildren && (
        <span
          role="button"
          tabIndex={-1}
          aria-label={treeRow.collapsed ? 'Expand replies' : 'Collapse replies'}
          title={treeRow.collapsed ? 'Expand replies' : 'Collapse replies'}
          onClick={(event) => {
            event.stopPropagation()
            onToggleCollapse?.(messageKey(treeRow.message))
          }}
          className="absolute flex size-4 items-center justify-center rounded-[3px] text-muted-foreground transition-colors hover:bg-[var(--hover-bg)] hover:text-foreground"
          style={isRoot ? { left: 0 } : { right: 0 }}
        >
          <Chevron size={13} strokeWidth={1.8} />
        </span>
      )}
    </span>
  )
}
