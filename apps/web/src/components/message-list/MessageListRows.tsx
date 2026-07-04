import { messageRowHeight } from '@/design'

import type { MessageSummary } from '@/api/types'
import type { EmailActions } from '@/hooks/useEmailActions'
import { useDesignTheme } from '@/hooks/useDesignTheme'

import { MessageRow } from '../MessageRow'
import type { ColumnId, ThreadListLayout } from '../thread-list/columns'
import type { ConversationTreeRow } from './conversationTree'
import type { MailboxDirectory } from './useMailboxDirectory'
import { MessageListErrorBanner } from './MessageListErrorBanner'
import { EmptyMessages, LoadingRows } from './MessageListStates'
import { messageKey, OVERSCAN_ROWS } from './model'

export interface MessageListErrorState {
  errorMessage: string
  isInvalidQueryError: boolean
  showClientQueryError: boolean
  showError: boolean
}

export function MessageListRows({
  actions,
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
  viewRole,
  viewportHeight,
  showSourceMailbox,
  mailboxDirectory,
  excludeMailboxId,
}: {
  actions: EmailActions
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
  viewRole: string | null
  viewportHeight: number
  /** Per-view "show source mailbox" toggle state (top-bar control). */
  showSourceMailbox: boolean
  mailboxDirectory: MailboxDirectory
  /** The mailbox already being viewed (single source-mailbox views), excluded
   *  from the chip's candidate memberships when possible. */
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
                actions={actions}
                viewRole={viewRole}
                onSelectMessage={onSelectRowMessage}
                onViewConversation={onViewConversation}
                treeRow={treeMode ? row : undefined}
                onToggleCollapse={onToggleCollapse}
                showSourceMailbox={showSourceMailbox}
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
