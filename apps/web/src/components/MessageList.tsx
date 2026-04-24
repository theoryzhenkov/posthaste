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
import { useQuery } from '@tanstack/react-query'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type { MouseEvent } from 'react'
import {
  applyAccountNamesToMessages,
  useAccountDirectory,
} from '../accountDirectory'
import { fetchSmartMailboxMessages, fetchSourceMessages } from '../api/client'
import type { DomainEvent, MessageSummary } from '../api/types'
import type { EmailActions } from '../hooks/useEmailActions'
import { MAIL_DOMAIN_EVENT_NAME } from '../hooks/useDaemonEvents'
import type { MailSelection } from '../mailState'
import { AlertCircle, Inbox, MousePointerClick } from 'lucide-react'
import type { SidebarSelection } from './Sidebar'
import { MessageRow } from './MessageRow'
import {
  type ColumnId,
  type SortConfig,
  buildThreadListLayout,
} from './thread-list/columns'
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
/** Per-view scroll offset cache to restore position on view switch. */
const scrollOffsetByView = new Map<string, number>()

function messageKey(message: MessageSummary): string {
  return `${message.sourceId}:${message.id}`
}

function selectionKey(selection: MailSelection | null): string | null {
  return selection ? `${selection.sourceId}:${selection.messageId}` : null
}

function viewKey(selectedView: SidebarSelection | null, searchQuery?: string) {
  const query = searchQuery ? `?q=${searchQuery}` : ''
  if (!selectedView) {
    return `none${query}`
  }
  if (selectedView.kind === 'smart-mailbox') {
    return `smart:${selectedView.id}${query}`
  }
  return `source:${selectedView.sourceId}:${selectedView.mailboxId}${query}`
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

function compareNullableText(a: string | null, b: string | null): number {
  return (a ?? '').localeCompare(b ?? '', undefined, { sensitivity: 'base' })
}

function compareBoolean(a: boolean, b: boolean): number {
  return Number(a) - Number(b)
}

function compareMessages(
  a: MessageSummary,
  b: MessageSummary,
  columnId: ColumnId,
): number {
  switch (columnId) {
    case 'date':
      return Date.parse(a.receivedAt) - Date.parse(b.receivedAt)
    case 'from':
      return compareNullableText(
        a.fromName ?? a.fromEmail,
        b.fromName ?? b.fromEmail,
      )
    case 'subject':
      return compareNullableText(a.subject, b.subject)
    case 'source':
      return compareNullableText(a.sourceName, b.sourceName)
    case 'flagged':
      return compareBoolean(a.isFlagged, b.isFlagged)
    case 'attachment':
      return compareBoolean(a.hasAttachment, b.hasAttachment)
    case 'unread':
      return compareBoolean(!a.isRead, !b.isRead)
    case 'preview':
      return compareNullableText(a.preview, b.preview)
    case 'tags':
      return compareNullableText(a.keywords.join(' '), b.keywords.join(' '))
  }
}

function sortMessages(messages: MessageSummary[], sort: SortConfig) {
  const direction = sort.direction === 'asc' ? 1 : -1
  return [...messages].sort((a, b) => {
    const primary = compareMessages(a, b, sort.columnId) * direction
    if (primary !== 0) {
      return primary
    }
    return messageKey(a).localeCompare(messageKey(b))
  })
}

function matchesSearch(message: MessageSummary, searchQuery?: string): boolean {
  const query = searchQuery?.trim().toLowerCase()
  if (!query) {
    return true
  }
  const haystack = [
    message.subject,
    message.preview,
    message.fromName,
    message.fromEmail,
    message.sourceName,
    message.keywords.join(' '),
  ]
    .filter(Boolean)
    .join(' ')
    .toLowerCase()
  return haystack.includes(query)
}

async function fetchMessagesForView(selectedView: SidebarSelection) {
  if (selectedView.kind === 'smart-mailbox') {
    return fetchSmartMailboxMessages(selectedView.id)
  }
  return fetchSourceMessages(selectedView.sourceId, selectedView.mailboxId)
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
    () => viewKey(selectedView, searchQuery),
    [selectedView, searchQuery],
  )
  const scrollContainerRef = useRef<HTMLDivElement>(null)
  const restoredViewKeyRef = useRef<string | null>(null)
  const [scrollTop, setScrollTop] = useState(0)
  const [viewportHeight, setViewportHeight] = useState(0)
  const accountDirectory = useAccountDirectory()

  const {
    data: rawMessages = [],
    isLoading,
    refetch,
    error,
  } = useQuery({
    queryKey: queryKeys.messages(selectedView),
    queryFn: () => fetchMessagesForView(selectedView!),
    enabled: selectedView !== null,
  })

  const displayMessages = useMemo(
    () => applyAccountNamesToMessages(rawMessages, accountDirectory),
    [accountDirectory, rawMessages],
  )

  const messages = useMemo(
    () =>
      sortMessages(
        displayMessages.filter((message) =>
          matchesSearch(message, searchQuery),
        ),
        sort,
      ),
    [displayMessages, searchQuery, sort],
  )
  const selectedKey = selectionKey(selection)

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
  }, [currentViewKey])

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
            {error && (
              <div className="flex flex-col items-center gap-3 px-3 py-12">
                <AlertCircle
                  size={32}
                  strokeWidth={1.5}
                  className="text-destructive/50"
                />
                <p className="text-sm text-destructive">
                  Failed to load messages
                </p>
                <button
                  type="button"
                  className="rounded border border-border px-3 py-1.5 text-xs text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
                  onClick={() => void refetch()}
                >
                  Try again
                </button>
              </div>
            )}
            {!isLoading && !error && messages.length === 0 && (
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
              </>
            )}
          </div>
        </div>
      </div>
    </div>
  )
}
