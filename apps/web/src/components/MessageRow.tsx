/**
 * Single message row in the message list.
 *
 * Renders sender, subject, preview, relative timestamp, unread dot,
 * flag star, attachment state, and source tag.
 *
 * @spec docs/L1-ui#messagelist
 */
import { Fragment, memo, useCallback } from 'react'
import {
  buildMessageContextActions,
  type MessageActionContext,
} from '../actions/contextualActions'
import type { MessageSummary } from '../api/types'
import type { EmailActions } from '../hooks/useEmailActions'
import { cn } from '../lib/utils'
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
}

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
}: MessageRowProps) {
  const messageRef = { messageId: message.id, sourceId: message.sourceId }
  const handleSelect = useCallback(() => {
    onSelectMessage(message)
  }, [message, onSelectMessage])
  const context: MessageActionContext = {
    message,
    target: messageRef,
    viewRole,
    surface: 'context-menu',
  }
  const contextActions = buildMessageContextActions(actions, context, {
    onOpen: handleSelect,
  })
  const row = (
    <button
      className={cn(
        'grid h-full w-full items-center gap-0',
        'text-left text-[13px] transition-colors',
        'ph-focus-ring',
        isSelected &&
          'bg-[var(--list-selection)] text-[var(--list-selection-foreground)]',
        !isSelected &&
          (isStriped
            ? 'bg-[var(--list-zebra-alt)] text-panel-foreground hover:bg-[var(--list-hover)]'
            : 'bg-[var(--list-zebra)] text-panel-foreground hover:bg-[var(--list-hover)]'),
      )}
      style={layout.gridStyle}
      onClick={handleSelect}
      onContextMenu={handleSelect}
      type="button"
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
