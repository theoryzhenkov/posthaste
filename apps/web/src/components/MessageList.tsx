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
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type { MouseEvent } from 'react'
import {
  applyAccountNamesToMessages,
  useAccountDirectory,
} from '../accountDirectory'
import { fetchSmartMailboxMessages, fetchSourceMessages } from '../api/client'
import { ApiError } from '../api/errors'
import type {
  DomainEvent,
  MessagePage,
  MessageSortField,
  MessageSummary,
} from '../api/types'
import type { EmailActions } from '../hooks/useEmailActions'
import { MAIL_DOMAIN_EVENT_NAME } from '../hooks/useDaemonEvents'
import type { MailSelection } from '../mailState'
import { AlertCircle, Inbox, MousePointerClick, X } from 'lucide-react'
import type { SidebarSelection } from './Sidebar'
import { MessageRow } from './MessageRow'
import { type SortConfig, buildThreadListLayout } from './thread-list/columns'
import { ThreadListHeader } from './thread-list/ThreadListHeader'
import { useColumnConfig } from './thread-list/useColumnConfig'
import { queryKeys } from '../queryKeys'

/** @spec docs/L1-ui#messagelist */
interface MessageListProps {
  selectedView: SidebarSelection | null
  selection: MailSelection | null
  onSelectMessage: (message: MailSelection) => void
  onClearSelection: () => void
  actions: EmailActions
  searchQuery?: string
}

/** @spec docs/L1-ui#messagelist */
const ROW_HEIGHT = 30
const OVERSCAN_ROWS = 6
const MESSAGE_PAGE_SIZE = 100
/** Per-view scroll offset cache to restore position on view switch. */
const scrollOffsetByView = new Map<string, number>()

function messageKey(message: MessageSummary): string {
  return `${message.sourceId}:${message.id}`
}

function selectionKey(selection: MailSelection | null): string | null {
  return selection ? `${selection.sourceId}:${selection.messageId}` : null
}

function viewKey(
  selectedView: SidebarSelection | null,
  searchQuery: string | undefined,
  sort: SortConfig,
) {
  const query = searchQuery ? `?q=${searchQuery}` : ''
  const sortKey = `#sort=${sort.columnId}:${sort.direction}`
  if (!selectedView) {
    return `none${query}${sortKey}`
  }
  if (selectedView.kind === 'smart-mailbox') {
    return `smart:${selectedView.id}${query}${sortKey}`
  }
  return `source:${selectedView.sourceId}:${selectedView.mailboxId}${query}${sortKey}`
}

function eventMayAffectView(
  payload: DomainEvent,
  selectedView: SidebarSelection | null,
): boolean {
  if (!selectedView) {
    return false
  }
  if (selectedView.kind === 'smart-mailbox') {
    return true
  }
  if (payload.accountId !== selectedView.sourceId) {
    return false
  }
  return (
    payload.mailboxId === null || payload.mailboxId === selectedView.mailboxId
  )
}

function serverSortField(sort: SortConfig): MessageSortField {
  switch (sort.columnId) {
    case 'date':
    case 'from':
    case 'subject':
    case 'source':
    case 'flagged':
    case 'attachment':
      return sort.columnId
    case 'unread':
    case 'preview':
    case 'tags':
      return 'date'
  }
}

function normalizedServerQuery(
  searchQuery: string | undefined,
): string | undefined {
  const query = searchQuery?.trim()
  return query ? query : undefined
}

async function fetchMessagesForView(
  selectedView: SidebarSelection,
  searchQuery: string | undefined,
  sort: SortConfig,
  cursor: string | null,
): Promise<MessagePage> {
  const q = normalizedServerQuery(searchQuery)
  const input = {
    q,
    cursor,
    limit: MESSAGE_PAGE_SIZE,
    sort: serverSortField(sort),
    sortDir: sort.direction,
  }
  if (selectedView.kind === 'smart-mailbox') {
    return fetchSmartMailboxMessages(selectedView.id, input)
  }
  return fetchSourceMessages(selectedView.sourceId, selectedView.mailboxId, {
    ...input,
  })
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
  actions,
  searchQuery,
}: MessageListProps) {
  const {
    columns,
    sort,
    widths,
    toggleColumn,
    reorderColumns,
    resetColumns,
    toggleSort,
    setColumnWidth,
  } = useColumnConfig()
  const tableLayout = useMemo(
    () => buildThreadListLayout(columns, widths),
    [columns, widths],
  )
  const currentViewKey = useMemo(
    () => viewKey(selectedView, searchQuery, sort),
    [selectedView, searchQuery, sort],
  )
  const scrollContainerRef = useRef<HTMLDivElement>(null)
  const restoredViewKeyRef = useRef<string | null>(null)
  const [scrollTop, setScrollTop] = useState(0)
  const [viewportHeight, setViewportHeight] = useState(0)
  const [dismissedErrorKey, setDismissedErrorKey] = useState<string | null>(
    null,
  )
  const accountDirectory = useAccountDirectory()

  const {
    data,
    isLoading,
    refetch,
    error,
    fetchNextPage,
    hasNextPage,
    isFetchingNextPage,
  } = useInfiniteQuery({
    queryKey: queryKeys.messages(selectedView, searchQuery, sort),
    queryFn: ({ pageParam }) =>
      fetchMessagesForView(selectedView!, searchQuery, sort, pageParam),
    enabled: selectedView !== null,
    initialPageParam: null as string | null,
    placeholderData: (previousData) => previousData,
    getNextPageParam: (lastPage) => lastPage.nextCursor,
  })

  const rawMessages = useMemo(
    () => data?.pages.flatMap((page) => page.items) ?? [],
    [data],
  )

  const displayMessages = useMemo(
    () => applyAccountNamesToMessages(rawMessages, accountDirectory),
    [accountDirectory, rawMessages],
  )

  const messages = displayMessages
  const selectedKey = selectionKey(selection)
  const errorKey = error ? `${currentViewKey}:${error.message}` : null
  const showError = Boolean(error && errorKey !== dismissedErrorKey)
  const errorMessage =
    error instanceof ApiError && error.code === 'invalid_query'
      ? `Search query is not valid: ${error.message}`
      : 'Failed to load messages'

  const navigateMessage = useCallback(
    (direction: 1 | -1) => {
      if (messages.length === 0) return

      const currentIndex = messages.findIndex(
        (message) => messageKey(message) === selectedKey,
      )
      const nextIndex =
        currentIndex === -1
          ? direction === 1
            ? 0
            : messages.length - 1
          : currentIndex + direction

      if (nextIndex < 0 || nextIndex >= messages.length) {
        return
      }

      const nextMessage = messages[nextIndex]
      onSelectMessage({
        conversationId: nextMessage.conversationId,
        sourceId: nextMessage.sourceId,
        messageId: nextMessage.id,
      })
    },
    [messages, onSelectMessage, selectedKey],
  )

  // Keyboard shortcuts -- suppressed when an input has focus.
  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      const target = event.target as HTMLElement
      if (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA') return
      if (event.metaKey || event.ctrlKey || event.altKey) return

      switch (event.key) {
        case 'j':
        case 'ArrowDown':
          event.preventDefault()
          navigateMessage(1)
          break
        case 'k':
        case 'ArrowUp':
          event.preventDefault()
          navigateMessage(-1)
          break
        case 'e':
          if (selection) {
            actions.archive({
              sourceId: selection.sourceId,
              messageId: selection.messageId,
            })
          }
          break
        case '#':
        case 'Backspace':
          if (selection) {
            actions.trash({
              sourceId: selection.sourceId,
              messageId: selection.messageId,
            })
          }
          break
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [actions, navigateMessage, selection])

  // Reset scroll-restore tracking on view change.
  useEffect(() => {
    restoredViewKeyRef.current = null
  }, [currentViewKey])

  // Restore scroll position when switching views.
  useEffect(() => {
    const node = scrollContainerRef.current
    if (!node || restoredViewKeyRef.current === currentViewKey) {
      return
    }
    const savedOffset = scrollOffsetByView.get(currentViewKey) ?? 0
    restoredViewKeyRef.current = currentViewKey
    node.scrollTop = savedOffset
    const frame = requestAnimationFrame(() => setScrollTop(savedOffset))
    return () => cancelAnimationFrame(frame)
  }, [currentViewKey, messages.length])

  // Track viewport height for virtualization.
  useEffect(() => {
    const node = scrollContainerRef.current
    if (!node) {
      return
    }

    const updateViewportHeight = () => setViewportHeight(node.clientHeight)
    updateViewportHeight()

    const resizeObserver = new ResizeObserver(updateViewportHeight)
    resizeObserver.observe(node)
    return () => resizeObserver.disconnect()
  }, [])

  // Listen for domain events and refresh messages.
  useEffect(() => {
    function handleDomainEvent(event: Event) {
      const payload = (event as CustomEvent<DomainEvent>).detail
      if (!eventMayAffectView(payload, selectedView)) {
        return
      }
      void refetch()
    }

    window.addEventListener(
      MAIL_DOMAIN_EVENT_NAME,
      handleDomainEvent as EventListener,
    )
    return () =>
      window.removeEventListener(
        MAIL_DOMAIN_EVENT_NAME,
        handleDomainEvent as EventListener,
      )
  }, [refetch, selectedView])

  const handleScroll = useCallback(() => {
    const node = scrollContainerRef.current
    if (!node) {
      return
    }
    setScrollTop(node.scrollTop)
    scrollOffsetByView.set(currentViewKey, node.scrollTop)
    const distanceToEnd = node.scrollHeight - node.scrollTop - node.clientHeight
    if (distanceToEnd < ROW_HEIGHT * 20 && hasNextPage && !isFetchingNextPage) {
      void fetchNextPage()
    }
  }, [currentViewKey, fetchNextPage, hasNextPage, isFetchingNextPage])

  useEffect(() => {
    const node = scrollContainerRef.current
    if (!node || !hasNextPage || isFetchingNextPage) {
      return
    }
    if (node.scrollHeight <= node.clientHeight + ROW_HEIGHT * 4) {
      void fetchNextPage()
    }
  }, [fetchNextPage, hasNextPage, isFetchingNextPage, messages.length])

  const handleBackgroundMouseDown = useCallback(
    (event: MouseEvent<HTMLDivElement>) => {
      if (event.button !== 0) {
        return
      }

      if (event.target === event.currentTarget) {
        onClearSelection()
        return
      }

      const target = event.target
      if (!(target instanceof HTMLElement)) {
        return
      }

      if (target.closest('[data-message-list-empty="true"]')) {
        onClearSelection()
      }
    },
    [onClearSelection],
  )

  if (!selectedView) {
    return (
      <div
        className="flex h-full flex-col items-center justify-center gap-3 bg-panel p-6"
        data-message-list-empty="true"
        onMouseDown={handleBackgroundMouseDown}
      >
        <MousePointerClick
          size={40}
          strokeWidth={1.5}
          className="text-muted-foreground/40"
        />
        <div className="text-center">
          <p className="text-sm font-medium text-muted-foreground">
            No mailbox selected
          </p>
          <p className="mt-1 text-xs text-muted-foreground/60">
            Pick a mailbox to get started
          </p>
        </div>
      </div>
    )
  }

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
  const topSpacerHeight = startIndex * ROW_HEIGHT
  const bottomSpacerHeight = (totalRows - endIndex) * ROW_HEIGHT
  const visibleMessages = messages.slice(startIndex, endIndex)

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
              onResetColumns={resetColumns}
              onResizeColumn={setColumnWidth}
              onReorderColumns={reorderColumns}
              onToggleColumn={toggleColumn}
              onToggleSort={toggleSort}
            />
          </div>

          <div
            ref={scrollContainerRef}
            className="ph-scroll min-h-0 flex-1 overflow-x-hidden overflow-y-auto bg-[var(--list-zebra)]"
            onMouseDown={handleBackgroundMouseDown}
            onScroll={handleScroll}
          >
            {isLoading && (
              <div
                className="space-y-0 bg-[var(--list-zebra)]"
                data-message-list-empty="true"
              >
                {Array.from({ length: 4 }).map((_, i) => (
                  <div
                    key={i}
                    className="border-b border-[var(--list-divider)] px-4 py-3"
                    style={{ height: ROW_HEIGHT }}
                  >
                    <div className="flex items-center gap-3">
                      <div className="h-3.5 w-28 animate-pulse rounded bg-muted" />
                      <div className="h-3 w-16 animate-pulse rounded bg-muted" />
                    </div>
                    <div className="mt-2.5 h-3 w-3/4 animate-pulse rounded bg-muted" />
                    <div className="mt-2 h-3 w-1/2 animate-pulse rounded bg-muted/60" />
                  </div>
                ))}
              </div>
            )}
            {showError && (
              <div className="border-b border-destructive/20 bg-destructive/5 px-3 py-2">
                <div className="flex items-start gap-2 text-sm text-destructive">
                  <AlertCircle size={16} strokeWidth={1.8} className="mt-0.5" />
                  <p className="min-w-0 flex-1">{errorMessage}</p>
                  <button
                    type="button"
                    className="grid size-6 shrink-0 place-items-center rounded text-destructive/70 transition-colors hover:bg-destructive/10 hover:text-destructive"
                    aria-label="Dismiss error"
                    onClick={() => setDismissedErrorKey(errorKey)}
                  >
                    <X size={14} />
                  </button>
                </div>
                <button
                  type="button"
                  className="mt-2 rounded border border-destructive/20 px-2 py-1 text-xs text-destructive transition-colors hover:bg-destructive/10"
                  onClick={() => void refetch()}
                >
                  Try again
                </button>
              </div>
            )}
            {!isLoading && !showError && messages.length === 0 && (
              <div
                className="flex flex-col items-center gap-3 px-3 py-12"
                data-message-list-empty="true"
              >
                <Inbox
                  size={40}
                  strokeWidth={1.5}
                  className="text-muted-foreground/40"
                />
                <div className="text-center">
                  <p className="text-sm font-medium text-muted-foreground">
                    No messages here yet
                  </p>
                  <p className="mt-1 text-xs text-muted-foreground/60">
                    Messages will appear as they arrive
                  </p>
                </div>
              </div>
            )}
            {messages.length > 0 && (
              <>
                <div
                  data-message-list-empty="true"
                  style={{ height: topSpacerHeight }}
                />
                {visibleMessages.map((message, index) => (
                  <div key={messageKey(message)} style={{ height: ROW_HEIGHT }}>
                    <MessageRow
                      message={message}
                      isSelected={messageKey(message) === selectedKey}
                      isStriped={(startIndex + index) % 2 === 1}
                      columns={columns}
                      layout={tableLayout}
                      actions={actions}
                      onSelect={() =>
                        onSelectMessage({
                          conversationId: message.conversationId,
                          sourceId: message.sourceId,
                          messageId: message.id,
                        })
                      }
                    />
                  </div>
                ))}
                <div
                  data-message-list-empty="true"
                  style={{ height: bottomSpacerHeight }}
                />
                {isFetchingNextPage && (
                  <div className="flex h-8 items-center justify-center">
                    <div className="size-3 animate-spin rounded-full border border-muted-foreground/30 border-t-muted-foreground" />
                  </div>
                )}
              </>
            )}
          </div>
        </div>
      </div>
    </div>
  )
}
