import type { MessageSummary } from '@/api/types'
import type { EmailActions } from '@/hooks/useEmailActions'

import { MessageRow } from '../MessageRow'
import type { ColumnId, ThreadListLayout } from '../thread-list/columns'
import { MessageListErrorBanner } from './MessageListErrorBanner'
import { EmptyMessages, LoadingRows } from './MessageListStates'
import { messageKey, OVERSCAN_ROWS, ROW_HEIGHT } from './model'

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
  layout,
  messages,
  onClearSearchQuery,
  onDismissError,
  onRetry,
  onSelectRowMessage,
  scrollTop,
  selectedKey,
  viewRole,
  viewportHeight,
}: {
  actions: EmailActions
  columns: ColumnId[]
  errorState: MessageListErrorState
  isFetchingNextPage: boolean
  isLoading: boolean
  layout: ThreadListLayout
  messages: MessageSummary[]
  onClearSearchQuery: () => void
  onDismissError: () => void
  onRetry: () => void
  onSelectRowMessage: (message: MessageSummary) => void
  scrollTop: number
  selectedKey: string | null
  viewRole: string | null
  viewportHeight: number
}) {
  const virtual = virtualizeMessages(messages, scrollTop, viewportHeight)

  return (
    <>
      {isLoading && <LoadingRows />}
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
      {!isLoading && !errorState.showError && messages.length === 0 && (
        <EmptyMessages />
      )}
      {messages.length > 0 && (
        <>
          <div
            data-message-list-empty="true"
            style={{ height: virtual.topSpacerHeight }}
          />
          {virtual.visibleMessages.map((message, index) => (
            <div key={messageKey(message)} style={{ height: ROW_HEIGHT }}>
              <MessageRow
                message={message}
                isSelected={messageKey(message) === selectedKey}
                isStriped={(virtual.startIndex + index) % 2 === 1}
                columns={columns}
                layout={layout}
                actions={actions}
                viewRole={viewRole}
                onSelectMessage={onSelectRowMessage}
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

function virtualizeMessages(
  messages: MessageSummary[],
  scrollTop: number,
  viewportHeight: number,
) {
  const totalRows = messages.length
  const safeViewportHeight = viewportHeight || ROW_HEIGHT * 8
  const startIndex = Math.max(
    0,
    Math.floor(scrollTop / ROW_HEIGHT) - OVERSCAN_ROWS,
  )
  const endIndex = Math.min(
    totalRows,
    Math.ceil((scrollTop + safeViewportHeight) / ROW_HEIGHT) + OVERSCAN_ROWS,
  )
  return {
    bottomSpacerHeight: (totalRows - endIndex) * ROW_HEIGHT,
    startIndex,
    topSpacerHeight: startIndex * ROW_HEIGHT,
    visibleMessages: messages.slice(startIndex, endIndex),
  }
}
