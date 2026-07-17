import type { MessageSortField, MessageSummary } from '@/api/types'
import type { MailListQuery } from '@/gen'
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
    case 'sourceMailbox':
    case 'tags':
      return 'date'
  }
}

/**
 * The `mailList` scope for a sidebar view + search + sort, without the
 * window (`limit`/`cursor` are the pager's concern). The prepared search
 * string rides as `freeText`.
 */
export function buildMailListQuery(
  selectedView: SidebarSelection,
  preparedSearchQuery: PreparedServerSearchQuery,
  sort: SortConfig,
): MailListQuery {
  const scope: MailListQuery =
    selectedView.kind === 'smart-mailbox'
      ? { smartMailboxId: selectedView.id }
      : {
          accountId: selectedView.sourceId,
          mailboxId: selectedView.mailboxId,
        }
  return {
    ...scope,
    freeText: preparedSearchQuery.query ?? null,
    sort: {
      field: serverSortField(sort),
      descending: sort.direction === 'desc',
    },
  }
}
