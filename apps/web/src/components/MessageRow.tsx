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
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from './ui/context-menu'
import {
  type ColumnId,
  type ThreadListLayout,
  getColumnDef,
} from './thread-list/columns'

/** @spec docs/L1-ui#messagelist */
interface MessageRowProps {
  message: MessageSummary
  isSelected: boolean
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
  /** Toggle a conversation's collapse state (conversation view only). */
  onToggleCollapse?: (conversationId: string) => void
}

/** Indent applied to reply rows, per depth level. */
const TREE_INDENT_PX = 22
/** Width reserved for the root chevron so root content aligns across rows. */
const TREE_CHEVRON_PX = 22

/**
 * Fixed-height message row displaying sender, subject,
 * preview, date, unread state, flag, and source.
 *
 * @spec docs/L1-ui#messagelist
 */
export const MessageRow = memo(function MessageRow({
  message,
  isSelected,
  isStriped,
  onSelectMessage,
  columns,
  layout,
  actions,
  viewRole,
  onViewConversation,
  treeRow,
  onToggleCollapse,
}: MessageRowProps) {
  const messageRef = { messageId: message.id, sourceId: message.sourceId }
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
          'bg-[var(--list-selection)] text-[var(--list-selection-foreground)]',
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
              {def.render(message)}
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
 * Leading gutter for conversation view: a collapse chevron on root rows and a
 * matching indent on reply rows, so children sit offset under their root.
 */
function TreeGutter({
  treeRow,
  onToggleCollapse,
}: {
  treeRow: ConversationTreeRow
  onToggleCollapse?: (conversationId: string) => void
}) {
  const indent = treeRow.depth * TREE_INDENT_PX
  const showChevron = treeRow.isRoot && treeRow.childCount > 0
  const Chevron = treeRow.collapsed ? ChevronRight : ChevronDown
  return (
    <span
      aria-hidden={!showChevron}
      className="flex h-full shrink-0 items-center justify-center"
      style={{ width: indent + TREE_CHEVRON_PX, paddingLeft: indent }}
    >
      {showChevron && (
        <span
          role="button"
          tabIndex={-1}
          aria-label={
            treeRow.collapsed ? 'Expand conversation' : 'Collapse conversation'
          }
          title={
            treeRow.collapsed ? 'Expand conversation' : 'Collapse conversation'
          }
          onClick={(event) => {
            event.stopPropagation()
            onToggleCollapse?.(treeRow.conversationId)
          }}
          className="flex size-4 items-center justify-center rounded-[3px] text-muted-foreground transition-colors hover:bg-[var(--hover-bg)] hover:text-foreground"
        >
          <Chevron size={13} strokeWidth={1.8} />
        </span>
      )}
    </span>
  )
}
