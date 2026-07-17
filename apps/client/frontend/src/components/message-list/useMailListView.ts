/**
 * The message list's data source: one windowed `mailList` query.
 *
 * The backend is the only evaluator — scope, filters, and sort all travel in
 * the query; the hook only grows the window (limit+cursor pages) for
 * infinite scroll. Liveness is the stream's job: a generation advance
 * invalidates every query, so the loaded window refetches in place.
 *
 * @spec docs/L1-ui#messagelist
 */
import { useCallback, useMemo } from 'react'
import { keepPreviousData, useInfiniteQuery } from '@tanstack/react-query'

import type { MessageSummary } from '@/api/types'
import { useMailClient } from '@/data/context'
import { fetchQuery } from '@/data/queries'
import { queryKeys } from '@/data/queryKeys'
import type { MailListResult } from '@/gen'
import type { PreparedServerSearchQuery } from '@/searchQuery'
import type { SidebarSelection } from '../Sidebar'
import type { SortConfig } from '../thread-list/columns'
import { MESSAGE_PAGE_SIZE, buildMailListQuery } from './model'

export interface MailListView {
  /** The loaded window's rows, newest page last. */
  items: MessageSummary[]
  /** True only on the first load of a view with no rows yet to show. */
  isLoading: boolean
  /**
   * A fatal load failure: there are no rows and no skeleton — the renderer
   * shows this with a retry affordance. Cleared on retry and on view change.
   */
  error: Error | null
  /** Refetch the view (the renderer's retry affordance). */
  retry: () => void
  /** Grow the window by a page; no-op while one is in flight. */
  loadMore: () => void
  /** Whether the answer reports more rows past the current window. */
  hasMore: boolean
  isLoadingMore: boolean
}

export function useMailListView({
  enabled,
  preparedSearchQuery,
  selectedView,
  sort,
}: {
  enabled: boolean
  preparedSearchQuery: PreparedServerSearchQuery
  selectedView: SidebarSelection | null
  sort: SortConfig
}): MailListView {
  const client = useMailClient()
  const active =
    enabled && selectedView !== null && !preparedSearchQuery.isBlocked

  const scope = useMemo(
    () =>
      selectedView
        ? buildMailListQuery(selectedView, preparedSearchQuery, sort)
        : null,
    [selectedView, preparedSearchQuery, sort],
  )

  const query = useInfiniteQuery({
    queryKey: queryKeys.mailList(scope ?? {}),
    queryFn: ({ pageParam }) =>
      fetchQuery<MailListResult>(client, {
        mailList: {
          ...(scope ?? {}),
          limit: MESSAGE_PAGE_SIZE,
          cursor: pageParam,
        },
      }),
    initialPageParam: null as string | null,
    getNextPageParam: (lastPage) => lastPage.nextCursor,
    enabled: active,
    // Keep the prior view's rows visible across a view switch until the new
    // answer lands (no skeleton flash on every sidebar click).
    placeholderData: keepPreviousData,
  })

  const items = useMemo(
    () => query.data?.pages.flatMap((page) => page.rows) ?? [],
    [query.data],
  )

  const { fetchNextPage, hasNextPage, isFetchingNextPage, refetch } = query
  const loadMore = useCallback(() => {
    if (hasNextPage && !isFetchingNextPage) {
      void fetchNextPage()
    }
  }, [fetchNextPage, hasNextPage, isFetchingNextPage])
  const retry = useCallback(() => {
    void refetch()
  }, [refetch])

  return {
    items,
    isLoading: active && query.isPending && !query.isPlaceholderData,
    error: query.error,
    retry,
    loadMore,
    hasMore: hasNextPage ?? false,
    isLoadingMore: isFetchingNextPage,
  }
}
