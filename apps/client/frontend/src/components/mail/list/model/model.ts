import type { MessageSortField, MessageSummary } from '@/data/transport/api'
import type { MailListQuery, MailListResult } from '@/gen'
import type { PreparedServerSearchQuery } from '@/domain/search'
import type { SidebarSelection } from '@/data/models/selection'
import type { MailClient } from '@/data/transport/client'
import { fetchQuery } from '@/data/queries/queries'
import type { SortConfig } from '../../thread/columns'

export const ROW_HEIGHT = 30
export const OVERSCAN_ROWS = 6
export const MESSAGE_PAGE_SIZE = 100
export const scrollOffsetByView = new Map<string, number>()

/** How many pages deep each view's window has grown, per canonical scope
 * key. Module-level for the same reason as scrollOffsetByView: a remounted
 * list must find its window again so scroll restoration has its rows. */
export const windowPagesByScope = new Map<string, number>()

/**
 * One live window over a mail list, fetched from the top in server-capped
 * chunks. The window — not each scroll page — is the unit react-query caches
 * and refetches: a deep-scrolled list is ONE query, so an invalidation costs
 * one refetch regardless of depth (refactor-ledger item 6; the accumulated
 * per-page queries refetched 11–13 times per mutation).
 *
 * Each chunk asks for the whole remainder; the backend clamps the limit to
 * its own page cap (MAX_LIST_LIMIT) and hands back a continuation cursor, so
 * the chunk size is server policy, never restated here. Cursors restart from
 * the top on every refetch, which keeps rows live and the scroll prefix
 * stable across invalidations.
 */
export async function fetchMailListWindow(
  client: MailClient,
  scope: MailListQuery,
  windowSize: number,
): Promise<MailListResult> {
  const rows: MessageSummary[] = []
  let cursor: string | null = null
  let nextCursor: string | null = null
  do {
    const page: MailListResult = await fetchQuery<MailListResult>(client, {
      mailList: { ...scope, limit: windowSize - rows.length, cursor },
    })
    rows.push(...page.rows)
    nextCursor = page.nextCursor
    if (page.rows.length === 0) break // a stuck cursor must not loop
    cursor = page.nextCursor
  } while (cursor !== null && rows.length < windowSize)
  return { rows, nextCursor }
}

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
