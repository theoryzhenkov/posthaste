/**
 * Single message row in the message list.
 *
 * Renders sender, subject, preview, relative timestamp, unread dot,
 * flag star, attachment state, and source tag.
 *
 */
import { Fragment, memo, useCallback } from 'react'
import { ChevronDown, ChevronRight } from 'lucide-react'
import type { Mailbox, MessageSummary } from '../../../data/transport/api/index'
import type { ResolvedActionView } from '../../../lib/command'
import { cn } from '../../../lib/design/cn'
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

/** Registry-resolved context menu for one row (built by the app shell via
 *  `commands/bind.buildRowContextMenu`); the row supplies its own callbacks
 *  and the account's mailbox read model at menu time. */
export type RowContextMenuFor = (input: {
  message: MessageSummary
  open: (message: MessageSummary) => void
  viewConversation: (message: MessageSummary) => void
  mailboxes: { list: (sourceId: string) => Mailbox[] }
}) => ResolvedActionView[]

interface MessageRowProps {
  message: MessageSummary
  /** The `j`/`k` SELECTION cursor sits on this row. */
  isSelected: boolean
  /** This row is the ACTIVE (opened) message the reader pane shows. */
  isActive?: boolean
  isPaneActive?: boolean
  isStriped: boolean
  onSelectMessage: (message: MessageSummary) => void
  columns: ColumnId[]
  layout: ThreadListLayout
  contextMenuFor: RowContextMenuFor
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
  isActive = false,
  isPaneActive = false,
  isStriped,
  onSelectMessage,
  columns,
  layout,
  contextMenuFor,
  onViewConversation,
  treeRow,
  onToggleCollapse,
  mailboxDirectory,
  excludeMailboxId,
}: MessageRowProps) {
  const renderContext: ColumnRenderContext = {
    mailboxDirectory,
    excludeMailboxId,
  }
  const handleSelect = useCallback(() => {
    onSelectMessage(message)
  }, [message, onSelectMessage])
  // The host-built resolver runs against this row's target: the row binds its
  // own open/view callbacks and the account's mailbox read model (already
  // subscribed once per account by useMailboxDirectory — options source for
  // the parameterized "Move to ▸"). Built per render — cheap plain objects.
  const contextActions = contextMenuFor({
    message,
    open: onSelectMessage,
    viewConversation: onViewConversation,
    mailboxes: { list: mailboxDirectory.list },
  })
  const row = (
    <button
      className={cn(
        'flex h-full w-full items-center gap-0',
        'text-left text-[13px] transition-colors',
        'ph-focus-ring',
        // The ACTIVE (opened) row carries the strong accent while the list is
        // the focused pane, greying out otherwise — accent means "focused",
        // exactly as the sidebar's itemButtonClass does for the active mailbox.
        isActive &&
          isPaneActive &&
          'bg-[var(--list-selection)] text-[var(--list-selection-foreground)]',
        isActive &&
          !isPaneActive &&
          'bg-[var(--list-selection-muted)] text-[var(--list-selection-muted-foreground)]',
        // A SELECTED-but-not-active row (the `j`/`k` cursor after it diverges
        // from the opened message — or before anything is opened) reuses the
        // sidebar's muted selection treatment, so the user always sees where
        // the next `j`/`k` lands.
        !isActive &&
          isSelected &&
          'bg-[var(--list-selection-muted)] text-[var(--list-selection-muted-foreground)]',
        !isActive &&
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
              {previous && previous.section !== action.section && (
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
                  variant={action.destructive ? 'destructive' : 'default'}
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
