import type { MessageSortField, MessageSummary } from '@/api/types'
import type { OperationContext } from '@/observability'
import type { RuntimeMessagePageRequest } from '@/runtime/types'
import type { PreparedServerSearchQuery } from '@/searchQuery'
import type { SidebarSelection } from '../Sidebar'
import type { SortConfig } from '../thread-list/columns'

export const ROW_HEIGHT = 30
export const OVERSCAN_ROWS = 6
export const MESSAGE_PAGE_SIZE = 100
export const scrollOffsetByView = new Map<string, number>()

export function messageKey(message: MessageSummary): string {
  return `${message.sourceId}:${message.id}`
}

export function selectionKey(
  selection: {
    sourceId: string
    messageId: string
  } | null,
): string | null {
  return selection ? `${selection.sourceId}:${selection.messageId}` : null
}

export function viewKey(
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

/**
 * Stable identity for a view's display mode (messages vs conversations),
 * independent of sort so toggling the mode survives re-sorting the same view.
 */
export function viewModeKey(
  selectedView: SidebarSelection | null,
  searchQuery: string | undefined,
): string {
  const query = searchQuery ? `?q=${searchQuery}` : ''
  if (!selectedView) {
    return `none${query}`
  }
  if (selectedView.kind === 'smart-mailbox') {
    return `smart:${selectedView.id}${query}`
  }
  return `source:${selectedView.sourceId}:${selectedView.mailboxId}${query}`
}

/**
 * Whether a view's rows are expected to span more than one source mailbox, so
 * the "show source mailbox" chip default should start ON. Smart mailboxes
 * (which realize unified/aggregate views like "All Inboxes") and any search
 * (global or mailbox-scoped) qualify; a single source mailbox with no search
 * modifier does not, since every row would repeat the same mailbox name.
 *
 * @spec docs/L1-ui#messagelist
 */
export function isAggregateMessageView(
  selectedView: SidebarSelection | null,
  searchQuery: string | undefined,
): boolean {
  if (selectedView?.kind === 'source-mailbox') {
    return Boolean(searchQuery && searchQuery.trim().length > 0)
  }
  return true
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

export function buildMessagePageRequest(
  selectedView: SidebarSelection,
  preparedSearchQuery: PreparedServerSearchQuery,
  sort: SortConfig,
  cursor: string | null,
  signal: AbortSignal,
  operation: OperationContext,
): RuntimeMessagePageRequest {
  return {
    scope:
      selectedView.kind === 'smart-mailbox'
        ? { kind: 'smart-mailbox', smartMailboxId: selectedView.id }
        : {
            kind: 'source-mailbox',
            sourceId: selectedView.sourceId,
            mailboxId: selectedView.mailboxId,
          },
    query: preparedSearchQuery.query,
    cursor,
    limit: MESSAGE_PAGE_SIZE,
    sort: serverSortField(sort),
    sortDir: sort.direction,
    signal,
    operation,
  }
}
