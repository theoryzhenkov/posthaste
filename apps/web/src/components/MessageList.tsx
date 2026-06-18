/**
 * Virtualized, message-first middle pane.
 *
 * The list displays individual messages by default. Thread viewing is a filter
 * concern: selecting a message still lets the reader load its surrounding
 * conversation, but the middle pane itself does not collapse rows by thread.
 *
 * @spec docs/L1-ui#messagelist
 * @spec docs/L1-ui#keyboard-shortcuts
 */
import { useInfiniteQuery } from '@tanstack/react-query'
import { useCallback, useMemo, useState } from 'react'
import type { MouseEvent } from 'react'

import {
  applyAccountNamesToMessages,
  useAccountDirectory,
} from '../accountDirectory'
import { ApiError } from '../api/errors'
import type { MessageSummary } from '../api/types'
import type { EmailActions } from '../hooks/useEmailActions'
import type { MailSelection } from '../mailState'
import { createOperationContext } from '../observability'
import { queryKeys } from '../queryKeys'
import type { PreparedServerSearchQuery } from '../searchQuery'
import {
  MessageListRows,
  type MessageListErrorState,
} from './message-list/MessageListRows'
import { NoMailboxSelected } from './message-list/MessageListStates'
import {
  fetchMessagesForView,
  selectionKey,
  viewKey,
} from './message-list/model'
import { useDomainEventRefresh } from './message-list/useDomainEventRefresh'
import { useMessageListNavigation } from './message-list/useMessageListNavigation'
import { useMessageListScroll } from './message-list/useMessageListScroll'
import type { SidebarSelection } from './Sidebar'
import { buildThreadListLayout } from './thread-list/columns'
import { ThreadListHeader } from './thread-list/ThreadListHeader'
import { useColumnConfig } from './thread-list/useColumnConfig'

/** @spec docs/L1-ui#messagelist */
interface MessageListProps {
  selectedView: SidebarSelection | null
  selection: MailSelection | null
  onSelectMessage: (message: MailSelection) => void
  onClearSelection: () => void
  onClearSearchQuery: () => void
  actions: EmailActions
  /** Mailbox role of the current view (null when ambiguous); drives row actions. */
  viewRole: string | null
  searchQuery?: string
  preparedSearchQuery: PreparedServerSearchQuery
}

/**
 * Message list panel: the middle column of the three-column layout.
 *
 * Handles individual-message loading, manual virtualization, live refresh on
 * domain events, per-view scroll restoration, and keyboard shortcuts.
 *
 * @spec docs/L1-ui#messagelist
 * @spec docs/L1-ui#keyboard-shortcuts
 */
export function MessageList({
  selectedView,
  selection,
  onSelectMessage,
  onClearSelection,
  onClearSearchQuery,
  actions,
  viewRole,
  searchQuery,
  preparedSearchQuery,
}: MessageListProps) {
  const columnConfig = useColumnConfig()
  const { columns, sort, widths } = columnConfig
  const tableLayout = useMemo(
    () => buildThreadListLayout(columns, widths),
    [columns, widths],
  )
  const currentViewKey = useMemo(
    () => viewKey(selectedView, searchQuery, sort),
    [selectedView, searchQuery, sort],
  )
  const operationEntry = useMemo(() => {
    const source =
      selectedView?.kind === 'smart-mailbox'
        ? 'message-list.smart-mailbox'
        : selectedView?.kind === 'source-mailbox'
          ? 'message-list.source-mailbox'
          : 'message-list'
    return {
      viewKey: currentViewKey,
      context: createOperationContext(
        preparedSearchQuery.query ? 'mail.search' : 'mail.list',
        source,
      ),
    }
  }, [currentViewKey, preparedSearchQuery.query, selectedView?.kind])
  const [dismissedErrorKey, setDismissedErrorKey] = useState<string | null>(
    null,
  )
  const accountDirectory = useAccountDirectory()

  const query = useInfiniteQuery({
    queryKey: queryKeys.messages(selectedView, searchQuery, sort),
    queryFn: ({ pageParam, signal }) =>
      fetchMessagesForView(
        selectedView!,
        preparedSearchQuery,
        sort,
        pageParam,
        signal,
        operationEntry.context,
      ),
    enabled: selectedView !== null && !preparedSearchQuery.isBlocked,
    initialPageParam: null as string | null,
    placeholderData: (previousData) => previousData,
    getNextPageParam: (lastPage) => lastPage.nextCursor,
  })

  const rawMessages = useMemo(
    () =>
      preparedSearchQuery.isBlocked
        ? []
        : (query.data?.pages.flatMap((page) => page.items) ?? []),
    [query.data, preparedSearchQuery.isBlocked],
  )
  const messages = useMemo(
    () => applyAccountNamesToMessages(rawMessages, accountDirectory),
    [accountDirectory, rawMessages],
  )
  const selectedKey = selectionKey(selection)
  const errorKey = query.error
    ? `${operationEntry.viewKey}:${query.error.message}`
    : null
  const errorState = buildErrorState({
    dismissedErrorKey,
    error: query.error,
    errorKey,
    preparedSearchQuery,
  })

  useMessageListNavigation({
    actions,
    currentViewKey,
    messages,
    onClearSelection,
    onSelectMessage,
    selectedKey,
    selection,
  })
  useDomainEventRefresh({
    isSearchBlocked: preparedSearchQuery.isBlocked,
    refetch: () => void query.refetch(),
    selectedView,
  })
  const { handleScroll, scrollContainerRef, scrollTop, viewportHeight } =
    useMessageListScroll({
      currentViewKey,
      fetchNextPage: () => void query.fetchNextPage(),
      hasNextPage: Boolean(query.hasNextPage),
      isFetchingNextPage: query.isFetchingNextPage,
      isSearchBlocked: preparedSearchQuery.isBlocked,
      messageCount: messages.length,
    })

  const handleBackgroundMouseDown = useCallback(
    (event: MouseEvent<HTMLDivElement>) => {
      if (event.button !== 0) return
      if (event.target === event.currentTarget) {
        onClearSelection()
        return
      }
      const target = event.target
      if (target instanceof HTMLElement) {
        if (target.closest('[data-message-list-empty="true"]')) {
          onClearSelection()
        }
      }
    },
    [onClearSelection],
  )

  const handleSelectRowMessage = useCallback(
    (message: MessageSummary) => onSelectMessage(toSelection(message)),
    [onSelectMessage],
  )

  if (!selectedView) {
    return <NoMailboxSelected onMouseDown={handleBackgroundMouseDown} />
  }

  return (
    <div className="flex h-full min-h-0 min-w-0 flex-col overflow-hidden bg-[var(--list-zebra)]">
      <div className="ph-scroll min-h-0 flex-1 overflow-x-auto overflow-y-hidden bg-[var(--list-zebra)]">
        <div
          className="flex h-full min-h-0 flex-col"
          style={tableLayout.tableStyle}
        >
          <div
            className="shrink-0 border-b border-border/80 bg-[var(--list-header)] text-panel-foreground"
            aria-label={
              searchQuery
                ? `Search results for ${searchQuery}`
                : selectedView.name
            }
          >
            <ThreadListHeader
              columns={columns}
              layout={tableLayout}
              sort={sort}
              widths={widths}
              onResetColumns={columnConfig.resetColumns}
              onResizeColumn={columnConfig.setColumnWidth}
              onReorderColumns={columnConfig.reorderColumns}
              onToggleColumn={columnConfig.toggleColumn}
              onToggleSort={columnConfig.toggleSort}
            />
          </div>
          <div
            ref={scrollContainerRef}
            className="ph-scroll min-h-0 flex-1 overflow-x-hidden overflow-y-auto bg-[var(--list-zebra)]"
            onMouseDown={handleBackgroundMouseDown}
            onScroll={handleScroll}
          >
            <MessageListRows
              actions={actions}
              columns={columns}
              errorState={errorState}
              isFetchingNextPage={query.isFetchingNextPage}
              isLoading={query.isLoading}
              layout={tableLayout}
              messages={messages}
              onClearSearchQuery={onClearSearchQuery}
              onDismissError={() => setDismissedErrorKey(errorKey)}
              onRetry={() => void query.refetch()}
              onSelectRowMessage={handleSelectRowMessage}
              scrollTop={scrollTop}
              selectedKey={selectedKey}
              viewRole={viewRole}
              viewportHeight={viewportHeight}
            />
          </div>
        </div>
      </div>
    </div>
  )
}

function toSelection(message: MessageSummary): MailSelection {
  return {
    conversationId: message.conversationId,
    sourceId: message.sourceId,
    messageId: message.id,
  }
}

function buildErrorState(input: {
  dismissedErrorKey: string | null
  error: Error | null
  errorKey: string | null
  preparedSearchQuery: PreparedServerSearchQuery
}): MessageListErrorState {
  const { dismissedErrorKey, error, errorKey, preparedSearchQuery } = input
  const showClientQueryError = preparedSearchQuery.isBlocked
  const showServerError = Boolean(
    !showClientQueryError && error && errorKey !== dismissedErrorKey,
  )
  const isInvalidQueryError =
    showClientQueryError ||
    (error instanceof ApiError && error.code === 'invalid_query')
  return {
    errorMessage:
      preparedSearchQuery.validation.state !== 'valid'
        ? `Search query is not valid: ${preparedSearchQuery.validation.message}`
        : error instanceof ApiError && error.code === 'invalid_query'
          ? `Search query is not valid: ${error.message}`
          : 'Failed to load messages',
    isInvalidQueryError,
    showClientQueryError,
    showError: showClientQueryError || showServerError,
  }
}
