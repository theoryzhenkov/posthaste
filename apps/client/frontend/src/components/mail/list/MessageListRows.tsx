import { messageRowHeight } from '@/lib/design'

import type { MessageSummary } from '@/data/transport/api'
import { useDesignTheme } from '@/lib/design/useDesignTheme'

import { MessageRow, type RowContextMenuFor } from './MessageRow'
import type { ColumnId, ThreadListLayout } from '../thread/columns'
import type { ConversationTreeRow } from './model/conversationTree'
import type { MailboxDirectory } from './model/useMailboxDirectory'
import { MessageListErrorBanner } from './MessageListErrorBanner'
import { EmptyMessages, LoadingRows } from './MessageListStates'
import { messageKey, OVERSCAN_ROWS } from './model/model'

export interface MessageListErrorState {
  errorMessage: string
  isInvalidQueryError: boolean
  showClientQueryError: boolean
  showError: boolean
}

export function MessageListRows({
  contextMenuFor,
  columns,
  errorState,
  isFetchingNextPage,
  isLoading,
  isSyncing,
  layout,
  onClearSearchQuery,
  onDismissError,
  onRetry,
  onSelectRowMessage,
  onViewConversation,
  onToggleCollapse,
  rows,
  treeMode,
  scrollTop,
  selectedKey,
  isPaneActive,
  viewportHeight,
  mailboxDirectory,
  excludeMailboxId,
}: {
  /** Registry-resolved row context menu (commands/bind). */
  contextMenuFor: RowContextMenuFor
  columns: ColumnId[]
  errorState: MessageListErrorState
  isFetchingNextPage: boolean
  isLoading: boolean
  isSyncing: boolean
  layout: ThreadListLayout
  rows: ConversationTreeRow[]
  treeMode: boolean
  onClearSearchQuery: () => void
  onDismissError: () => void
  onRetry: () => void
  onSelectRowMessage: (message: MessageSummary) => void
  onViewConversation: (message: MessageSummary) => void
  onToggleCollapse: (messageKey: string) => void
  scrollTop: number
  selectedKey: string | null
  isPaneActive: boolean
  viewportHeight: number
  /** Cache-only mailbox resolver, consumed by the `sourceMailbox` column cell. */
  mailboxDirectory: MailboxDirectory
  /** The mailbox already being viewed (single source-mailbox views), excluded
   *  from the `sourceMailbox` cell's candidate memberships when possible. */
  excludeMailboxId: string | null
}) {
  const rowHeight = messageRowHeight(useDesignTheme().density)
  const virtual = virtualizeRows(rows, scrollTop, viewportHeight, rowHeight)

  return (
    <>
      {isLoading && <LoadingRows rowHeight={rowHeight} />}
      {errorState.showError && (
        <MessageListErrorBanner
          errorMessage={errorState.errorMessage}
          isInvalidQueryError={errorState.isInvalidQueryError}
          showClientQueryError={errorState.showClientQueryError}
          onClearSearchQuery={onClearSearchQuery}
          onDismiss={onDismissError}
          onRetry={onRetry}
        />
      )}
      {!isLoading && !errorState.showError && rows.length === 0 && (
        <EmptyMessages isSyncing={isSyncing} />
      )}
      {rows.length > 0 && (
        <>
          <div
            data-message-list-empty="true"
            style={{ height: virtual.topSpacerHeight }}
          />
          {virtual.visibleRows.map((row, index) => (
            <div
              key={`${row.conversationId}:${messageKey(row.message)}:${row.depth}`}
              style={{ height: rowHeight }}
            >
              <MessageRow
                message={row.message}
                isSelected={messageKey(row.message) === selectedKey}
                isPaneActive={isPaneActive}
                isStriped={(virtual.startIndex + index) % 2 === 1}
                columns={columns}
                layout={layout}
                contextMenuFor={contextMenuFor}
                onSelectMessage={onSelectRowMessage}
                onViewConversation={onViewConversation}
                treeRow={treeMode ? row : undefined}
                onToggleCollapse={onToggleCollapse}
                mailboxDirectory={mailboxDirectory}
                excludeMailboxId={excludeMailboxId}
              />
            </div>
          ))}
          <div
            data-message-list-empty="true"
            style={{ height: virtual.bottomSpacerHeight }}
          />
          {isFetchingNextPage && (
            <div className="flex h-8 items-center justify-center">
              <div className="size-3 animate-spin rounded-full border border-muted-foreground/30 border-t-muted-foreground" />
            </div>
          )}
        </>
      )}
    </>
  )
}

function virtualizeRows(
  rows: ConversationTreeRow[],
  scrollTop: number,
  viewportHeight: number,
  rowHeight: number,
) {
  const totalRows = rows.length
  const safeViewportHeight = viewportHeight || rowHeight * 8
  const startIndex = Math.max(
    0,
    Math.floor(scrollTop / rowHeight) - OVERSCAN_ROWS,
  )
  const endIndex = Math.min(
    totalRows,
    Math.ceil((scrollTop + safeViewportHeight) / rowHeight) + OVERSCAN_ROWS,
  )
  return {
    bottomSpacerHeight: (totalRows - endIndex) * rowHeight,
    startIndex,
    topSpacerHeight: startIndex * rowHeight,
    visibleRows: rows.slice(startIndex, endIndex),
  }
}
