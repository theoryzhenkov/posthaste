/**
 * Single message row in the message list.
 *
 * Renders sender, subject, preview, relative timestamp, unread dot,
 * flag star, attachment state, and source tag.
 *
 * @spec docs/L1-ui#messagelist
 */
import { Archive, Eye, EyeOff, MailOpen, Star, Trash2 } from 'lucide-react'
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
  onSelect: () => void
  columns: ColumnId[]
  layout: ThreadListLayout
  actions: EmailActions
}

/**
 * Fixed-height message row displaying sender, subject,
 * preview, date, unread state, flag, and source.
 *
 * @spec docs/L1-ui#messagelist
 */
export function MessageRow({
  message,
  isSelected,
  isStriped,
  onSelect,
  columns,
  layout,
  actions,
}: MessageRowProps) {
  const messageRef = { messageId: message.id, sourceId: message.sourceId }
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
      onClick={onSelect}
      onContextMenu={onSelect}
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
        <ContextMenuItem onSelect={onSelect}>
          <MailOpen size={14} />
          Open
        </ContextMenuItem>
        <ContextMenuItem onSelect={() => actions.toggleRead(message)}>
          {message.isRead ? <EyeOff size={14} /> : <Eye size={14} />}
          {message.isRead ? 'Mark unread' : 'Mark read'}
        </ContextMenuItem>
        <ContextMenuItem onSelect={() => actions.toggleFlag(message)}>
          <Star size={14} />
          {message.isFlagged ? 'Unflag' : 'Flag'}
        </ContextMenuItem>
        <ContextMenuSeparator />
        <ContextMenuItem onSelect={() => actions.archive(messageRef)}>
          <Archive size={14} />
          Archive
        </ContextMenuItem>
        <ContextMenuItem
          variant="destructive"
          onSelect={() => actions.trash(messageRef)}
        >
          <Trash2 size={14} />
          Move to Trash
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  )
}
