/**
 * Single conversation row in the message list.
 *
 * Renders sender, subject, preview, relative timestamp, unread dot,
 * flag star, message count badge, and source tag.
 *
 * @spec docs/L1-ui#messagelist
 */
import type { ConversationSummary } from '../api/types'
import { cn } from '../lib/utils'
import {
  type ColumnId,
  type ThreadListLayout,
  getColumnDef,
} from './thread-list/columns'

/** @spec docs/L1-ui#messagelist */
interface MessageRowProps {
  message: ConversationSummary
  isSelected: boolean
  isStriped: boolean
  onSelect: () => void
  columns: ColumnId[]
  layout: ThreadListLayout
}

/**
 * Fixed-height conversation row displaying sender, subject,
 * preview, date, unread state, flag, and thread count.
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
}: MessageRowProps) {
  return (
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
}
