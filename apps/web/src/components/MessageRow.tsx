/**
 * Single message row in the message list.
 *
 * Renders sender, subject, preview, relative timestamp, unread dot,
 * flag star, attachment state, and source tag.
 *
 * @spec docs/L1-ui#messagelist
 */
import { Fragment, memo, useCallback } from 'react'
import { ChevronDown, ChevronRight } from 'lucide-react'
import {
  buildMessageContextActions,
  type MessageActionContext,
} from '../actions/contextualActions'
import type { MessageSummary } from '../api/types'
import type { EmailActions } from '../hooks/useEmailActions'
import { cn } from '../lib/utils'
import type { ConversationTreeRow } from './message-list/conversationTree'
import { messageKey } from './message-list/model'
import type { MailboxDirectory } from './message-list/useMailboxDirectory'
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from './ui/context-menu'
import {
  type ColumnId,
  type ColumnRenderContext,
  type ThreadListLayout,
  getColumnDef,
} from './thread-list/columns'

/** @spec docs/L1-ui#messagelist */
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
 *
 * @spec docs/L1-ui#messagelist
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
  const handleViewConversation = useCallback(() => {
    onViewConversation(message)
  }, [message, onViewConversation])
  const context: MessageActionContext = {
    message,
    target: messageRef,
    viewRole,
    surface: 'context-menu',
  }
  const contextActions = buildMessageContextActions(actions, context, {
    onOpen: handleSelect,
    onViewConversation: handleViewConversation,
  })
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
            <Fragment key={action.id}>
              {previous && previous.group !== action.group && (
                <ContextMenuSeparator />
              )}
              <ContextMenuItem
                variant={action.destructive ? 'destructive' : 'default'}
                onSelect={action.run}
              >
                <Icon size={14} />
                {action.title}
              </ContextMenuItem>
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
