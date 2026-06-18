import type {
  DomainEvent,
  MessagePage,
  MessageSortField,
  MessageSummary,
} from '@/api/types'
import { messagePageClient } from '@/messagePageClient'
import type { OperationContext } from '@/observability'
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

export function eventMayAffectView(
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

export async function fetchMessagesForView(
  selectedView: SidebarSelection,
  preparedSearchQuery: PreparedServerSearchQuery,
  sort: SortConfig,
  cursor: string | null,
  signal: AbortSignal,
  operation: OperationContext,
): Promise<MessagePage> {
  return messagePageClient.fetchPage({
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
  })
}
