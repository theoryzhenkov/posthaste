/**
 * Virtualized, message-first middle pane.
 *
 * The list displays individual messages by default. Thread viewing is a filter
 * concern: selecting a message still lets the reader load its surrounding
 * conversation, but the middle pane itself does not collapse rows by thread.
 *
 */
import { useCallback, useMemo, useState } from 'react'
import type { MouseEvent } from 'react'

import {
  applyAccountNamesToMessages,
  useAccountDirectory,
} from '../accountDirectory'
import type { MessageSummary } from '../api/types'
import { MailApiError } from '@/client'
import type { EmailActions } from '../hooks/useEmailActions'
import type { MailSelection } from '@/data/selection'
import { useActivePane } from './keyboard/usePane'
import type { PreparedServerSearchQuery } from '../searchQuery'
import { flatMessageRows } from './message-list/conversationTree'
import {
  MessageListRows,
  type MessageListErrorState,
} from './message-list/MessageListRows'
import { NoMailboxSelected } from './message-list/MessageListStates'
import {
  messageKey,
  selectionKey,
  viewKey,
  viewModeKey,
} from './message-list/model'
import { useConversationTree } from './message-list/useConversationTree'
import { useMailboxDirectory } from './message-list/useMailboxDirectory'
import { useMailListView } from './message-list/useMailListView'
import { useMessageListNavigation } from './message-list/useMessageListNavigation'
import { useMessageListScroll } from './message-list/useMessageListScroll'
import { useViewMode } from './message-list/useViewMode'
import type { SidebarSelection } from './Sidebar'
import { buildThreadListLayout } from './thread-list/columns'
import { ThreadListHeader } from './thread-list/ThreadListHeader'
import { useColumnConfig } from './thread-list/useColumnConfig'

interface MessageListProps {
  selectedView: SidebarSelection | null
  selection: MailSelection | null
  onSelectMessage: (message: MailSelection) => void
  onClearSelection: () => void
  onClearSearchQuery: () => void
  actions: EmailActions
  /** Mailbox role of the current view (null when ambiguous); drives row actions. */
  viewRole: string | null
  /** Filter the view to a message's conversation (contextual action). */
  onViewConversation: (message: MessageSummary) => void
  searchQuery?: string
  preparedSearchQuery: PreparedServerSearchQuery
}

/**
 * Message list panel: the middle column of the three-column layout.
 *
 * Handles individual-message loading, manual virtualization, live refresh on
 * domain events, per-view scroll restoration, and keyboard shortcuts.
 */
export function MessageList({
  selectedView,
  selection,
  onSelectMessage,
  onClearSelection,
  onClearSearchQuery,
  actions,
  viewRole,
  onViewConversation,
  searchQuery,
  preparedSearchQuery,
}: MessageListProps) {
  const columnConfig = useColumnConfig()
  const { columns, sort, widths } = columnConfig
  const tableLayout = useMemo(
    () => buildThreadListLayout(columns, widths),
    [columns, widths],
  )
  const viewModeKeyValue = useMemo(
    () => viewModeKey(selectedView, searchQuery),
    [selectedView, searchQuery],
  )
  const { mode } = useViewMode(viewModeKeyValue)
  const treeMode = mode === 'conversations'
  const excludeMailboxId =
    selectedView?.kind === 'source-mailbox' ? selectedView.mailboxId : null
  const currentViewKey = useMemo(
    () => `${viewKey(selectedView, searchQuery, sort)}#mode=${mode}`,
    [selectedView, searchQuery, sort, mode],
  )
  const [dismissedErrorKey, setDismissedErrorKey] = useState<string | null>(
    null,
  )
  const accountDirectory = useAccountDirectory()
  const mailboxDirectory = useMailboxDirectory(accountDirectory)

  // Single source for the list: one windowed `mailList` query. The hook owns
  // the rows, loading, window-extend, and retry; the component renders what
  // it returns.
  const mailListView = useMailListView({
    enabled: true,
    preparedSearchQuery,
    selectedView,
    sort,
  })

  const rawMessages = useMemo(
    () => (preparedSearchQuery.isBlocked ? [] : mailListView.items),
    [mailListView.items, preparedSearchQuery.isBlocked],
  )
  const messages = useMemo(
    () => applyAccountNamesToMessages(rawMessages, accountDirectory),
    [accountDirectory, rawMessages],
  )
  // Conversation view groups the same messages into a two-level tree; the flat
  // list maps straight through. Both feed one renderer + the same navigation.
  const conversationTree = useConversationTree({
    anchors: messages,
    enabled: treeMode && !preparedSearchQuery.isBlocked,
    accountDirectory,
  })
  const rows = useMemo(
    () => (treeMode ? conversationTree.rows : flatMessageRows(messages)),
    [treeMode, conversationTree.rows, messages],
  )
  const navMessages = treeMode ? conversationTree.visibleMessages : messages
  // Whether the view's account(s) are mid-sync, so an empty list shows a
  // "Syncing…" state instead of bare "no messages" (e.g. during a post-repair
  // full re-sync, where the projection is legitimately empty while mail loads).
  // No accounts configured at all → the empty list is onboarding, not an empty
  // mailbox; the empty state offers an "Add an account" CTA instead.
  const hasNoAccounts = accountDirectory.accounts.length === 0
  const isSyncing = useMemo(() => {
    const accounts = accountDirectory.accounts
    if (selectedView?.kind === 'source-mailbox') {
      return (
        accounts.find((account) => account.id === selectedView.sourceId)
          ?.status === 'syncing'
      )
    }
    return accounts.some(
      (account) => account.enabled && account.status === 'syncing',
    )
  }, [accountDirectory.accounts, selectedView])
  const selectedKey = selectionKey(selection)

  const { activePane } = useActivePane()
  const isListActive = activePane === 'list'

  const handleSelectRowMessage = useCallback(
    (message: MessageSummary) => onSelectMessage(toSelection(message)),
    [onSelectMessage],
  )

  // A fatal load failure surfaces here as an inline error + retry (instead
  // of an infinite skeleton); search-syntax errors still flow through
  // `buildErrorState` via `preparedSearchQuery`.
  const loadError = mailListView.error
  const errorKey: string | null = loadError ? currentViewKey : null
  const errorState = buildErrorState({
    dismissedErrorKey,
    error: loadError,
    errorKey,
    preparedSearchQuery,
  })

  // The focused tree row (conversation view only), so `h`/`l` can collapse/
  // expand exactly the node the user is on. Collapse is keyed by the node's
  // message key, which is also `selectedKey`.
  const focusedRow =
    treeMode && selectedKey
      ? conversationTree.rows.find(
          (row) => messageKey(row.message) === selectedKey,
        )
      : undefined
  useMessageListNavigation({
    currentViewKey,
    messages: navMessages,
    onClearSelection,
    onSelectMessage,
    selectedKey,
    onCollapseFocused: treeMode
      ? () => {
          if (selectedKey && focusedRow?.hasChildren && !focusedRow.collapsed) {
            conversationTree.toggleCollapse(selectedKey)
          }
        }
      : undefined,
    onExpandFocused: treeMode
      ? () => {
          if (selectedKey && focusedRow?.hasChildren && focusedRow.collapsed) {
            conversationTree.toggleCollapse(selectedKey)
          }
        }
      : undefined,
  })
  const { handleScroll, scrollContainerRef, scrollTop, viewportHeight } =
    useMessageListScroll({
      currentViewKey,
      fetchNextPage: () => {
        mailListView.loadMore()
      },
      hasNextPage: mailListView.hasMore,
      isFetchingNextPage: mailListView.isLoadingMore,
      isSearchBlocked: preparedSearchQuery.isBlocked,
      messageCount: rows.length,
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

  if (!selectedView) {
    return (
      <NoMailboxSelected
        onMouseDown={handleBackgroundMouseDown}
        hasNoAccounts={hasNoAccounts}
      />
    )
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
              isFetchingNextPage={mailListView.isLoadingMore}
              isLoading={mailListView.isLoading}
              isSyncing={isSyncing}
              layout={tableLayout}
              onClearSearchQuery={onClearSearchQuery}
              onDismissError={() => setDismissedErrorKey(errorKey)}
              onRetry={() => mailListView.retry()}
              onSelectRowMessage={handleSelectRowMessage}
              onViewConversation={onViewConversation}
              onToggleCollapse={conversationTree.toggleCollapse}
              rows={rows}
              treeMode={treeMode}
              scrollTop={scrollTop}
              selectedKey={selectedKey}
              isPaneActive={isListActive}
              viewRole={viewRole}
              viewportHeight={viewportHeight}
              mailboxDirectory={mailboxDirectory}
              excludeMailboxId={excludeMailboxId}
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
    (error instanceof MailApiError && error.kind === 'malformedRequest')
  return {
    errorMessage:
      preparedSearchQuery.validation.state !== 'valid'
        ? `Search query is not valid: ${preparedSearchQuery.validation.message}`
        : error instanceof MailApiError && error.kind === 'malformedRequest'
          ? `Search query is not valid: ${error.message}`
          : 'Failed to load messages',
    isInvalidQueryError,
    showClientQueryError,
    showError: showClientQueryError || showServerError,
  }
}
