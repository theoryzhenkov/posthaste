/**
 * The message list's data source: ONE windowed `mailList` query.
 *
 * The backend is the only evaluator — scope, filters, and sort all travel in
 * the query; the hook only grows the window (a limit that rises by
 * MESSAGE_PAGE_SIZE per scroll page, fetched via fetchMailListWindow) for
 * infinite scroll. Liveness is the stream's job: a generation advance
 * invalidates every query, and because the whole window is one cache entry
 * the deep-scrolled list refetches exactly once — never once per scroll page
 * (refactor-ledger item 6). Growing the window switches to a new cache key
 * whose placeholder is the previous window, so the loaded rows stay on
 * screen and the scroll position holds.
 */
import { useCallback, useMemo, useState } from 'react'
import { keepPreviousData, useQuery } from '@tanstack/react-query'

import type { MessageSummary } from '@/data/transport/api'
import { useMailClient } from '@/data/context'
import { queryKeys } from '@/data/queries/queryKeys'
import type { PreparedServerSearchQuery } from '@/domain/search'
import type { SidebarSelection } from '@/data/models/selection'
import type { SortConfig } from '../../thread/columns'
import {
  MESSAGE_PAGE_SIZE,
  buildMailListQuery,
  fetchMailListWindow,
  windowPagesByScope,
} from './model'

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
  const scopeKey = useMemo(() => queryKeys.mailList(scope ?? {})[1], [scope])

  // The window depth in pages: state for the current scope, restored from
  // the per-view map on a scope change (derived-state reset, no effect).
  const [win, setWin] = useState({ scopeKey, pages: windowPagesByScope.get(scopeKey) ?? 1 })
  const pages = win.scopeKey === scopeKey ? win.pages : (windowPagesByScope.get(scopeKey) ?? 1)
  const windowSize = pages * MESSAGE_PAGE_SIZE

  const query = useQuery({
    queryKey: queryKeys.mailList({ ...(scope ?? {}), limit: windowSize }),
    queryFn: () => fetchMailListWindow(client, scope ?? {}, windowSize),
    enabled: active,
    // Keep the prior answer's rows visible — across a view switch until the
    // new answer lands (no skeleton flash on every sidebar click), and
    // across a window grow until the larger window lands (no scroll jump).
    placeholderData: keepPreviousData,
  })

  const items = useMemo(() => query.data?.rows ?? [], [query.data])
  const hasMore = query.data?.nextCursor != null
  const { refetch, isFetching } = query

  const loadMore = useCallback(() => {
    if (hasMore && !isFetching) {
      const next = pages + 1
      windowPagesByScope.set(scopeKey, next)
      setWin({ scopeKey, pages: next })
    }
  }, [hasMore, isFetching, pages, scopeKey])
  const retry = useCallback(() => {
    void refetch()
  }, [refetch])

  return {
    items,
    isLoading: active && query.isPending && !query.isPlaceholderData,
    error: query.error,
    retry,
    loadMore,
    hasMore,
    // A grow in flight: the placeholder shows the previous, smaller window
    // of the SAME scope (a view switch leaves win.scopeKey stale instead).
    isLoadingMore:
      query.isFetching && query.isPlaceholderData && win.scopeKey === scopeKey && pages > 1,
  }
}
