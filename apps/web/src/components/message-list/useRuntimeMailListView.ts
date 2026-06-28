import { useCallback, useEffect, useMemo, useRef, useState } from 'react'

import {
  useQuery,
  useQueryClient,
  type InfiniteData,
} from '@tanstack/react-query'

import type { MessagePage, MessageSummary } from '@/api/types'
import { runtimeSessionClient } from '@/runtime/sessionClient'
import type {
  RuntimeMailListDelta,
  RuntimeMailListViewState,
  RuntimeViewSnapshot,
} from '@/runtime/types'
import { LOG_EVENTS, uiLogger } from '@/logger'
import type { OperationContext } from '@/observability'
import type { PreparedServerSearchQuery } from '@/searchQuery'
import type { SidebarSelection } from '../Sidebar'
import type { SortConfig } from '../thread-list/columns'
import { MESSAGE_PAGE_SIZE, buildMessagePageRequest } from './model'

function pageFromViewState(state: RuntimeMailListViewState): MessagePage {
  return {
    items: state.rows.map((row) => row.projection),
    nextCursor: state.continuation.afterCursor,
  }
}

function applySnapshotToQueryData(
  snapshot: RuntimeViewSnapshot<RuntimeMailListViewState>,
): InfiniteData<MessagePage, string | null> {
  return {
    pages: [pageFromViewState(snapshot.data)],
    pageParams: [null],
  }
}

/** The runtime keys mail-list rows by `${sourceId}:${id}`. */
function rowKeyOf(item: MessagePage['items'][number]): string {
  return `${item.sourceId}:${item.id}`
}

/**
 * Reconcile an incremental mail-list delta (replication client-link) against the held
 * single-page window: when `order` is present, drop rows whose key is absent and
 * reorder to it; then apply `upserts` by `rowKey`. Produces exactly the rows a
 * whole `viewReplace` would, from a fraction of the payload.
 */
function applyDeltaToQueryData(
  current: InfiniteData<MessagePage, string | null> | undefined,
  delta: RuntimeMailListDelta,
): InfiniteData<MessagePage, string | null> | undefined {
  if (!current || current.pages.length === 0) {
    return current
  }
  const page = current.pages[0]
  const upsertByKey = new Map(
    delta.upserts.map((row) => [row.rowKey, row.projection] as const),
  )
  let items: MessagePage['items']
  if (delta.order) {
    const heldByKey = new Map(
      page.items.map((item) => [rowKeyOf(item), item] as const),
    )
    items = delta.order
      .map((key) => upsertByKey.get(key) ?? heldByKey.get(key))
      .filter((item): item is MessagePage['items'][number] => item != null)
  } else {
    items = page.items.map((item) => upsertByKey.get(rowKeyOf(item)) ?? item)
  }
  return {
    ...current,
    pages: [{ ...page, items }, ...current.pages.slice(1)],
  }
}

export interface RuntimeMailListView {
  /** The current window's rows (message projections), reactive to view frames. */
  items: MessageSummary[]
  /** True only on the first load of a view with no rows yet to show. */
  isLoading: boolean
  /**
   * A fatal open-path failure: the view never opened, so there are no rows and
   * no skeleton — the renderer shows this with a retry affordance. Cleared on
   * retry and on view change.
   */
  error: Error | null
  /** Re-open the view (the renderer's retry affordance). */
  retry: () => void
  /** Grow the open view's window by a page; no-op while one is in flight. */
  loadMore: () => void
  /** Whether the runtime view reports more rows past the current window. */
  hasMore: boolean
  isLoadingMore: boolean
}

/**
 * Renders a mail list from a runtime `mailList` view: opens the view, feeds its
 * snapshot + `viewReplace` frames into the query cache, and grows the window in
 * place via the runtime extend operation for infinite scroll.
 *
 * @spec docs/runtime/adapter/L2#view-operation-flow
 */
export function useRuntimeMailListView({
  enabled,
  operation,
  preparedSearchQuery,
  queryKey,
  selectedView,
  sort,
}: {
  enabled: boolean
  operation: OperationContext
  preparedSearchQuery: PreparedServerSearchQuery
  queryKey: readonly unknown[]
  selectedView: SidebarSelection | null
  sort: SortConfig
}): RuntimeMailListView {
  const queryClient = useQueryClient()
  const viewIdRef = useRef<string | undefined>(undefined)
  const loadingMoreRef = useRef(false)
  const [hasMore, setHasMore] = useState(false)
  const [isLoadingMore, setIsLoadingMore] = useState(false)
  const [retryNonce, setRetryNonce] = useState(0)
  const [error, setError] = useState<Error | null>(null)
  const retry = useCallback(() => {
    setError(null)
    setRetryNonce((n) => n + 1)
  }, [])

  // The mail-list rows live in the query cache (written by this hook from view
  // frames); this read makes them reactive without a second, fetching query.
  // The view is runtime-fed, so the queryFn must never run (enabled: false);
  // `placeholderData` keeps the prior view's rows visible across a view switch
  // until the new snapshot lands (the old HTTP query's behavior).
  const cached = useQuery<InfiniteData<MessagePage, string | null>>({
    queryKey,
    queryFn: () => {
      throw new Error('mail-list view is runtime-fed; it must not be fetched')
    },
    enabled: false,
    placeholderData: (previous) => previous,
  })
  const items = useMemo(
    () => cached.data?.pages.flatMap((page) => page.items) ?? [],
    [cached.data],
  )
  const isLoading =
    enabled &&
    selectedView !== null &&
    !preparedSearchQuery.isBlocked &&
    error === null &&
    cached.data === undefined

  useEffect(() => {
    if (!enabled || !selectedView || preparedSearchQuery.isBlocked) {
      return
    }

    let closed = false
    let viewId: string | undefined
    let unsubscribe: (() => void) | undefined
    const abort = new AbortController()
    const sourceId =
      selectedView.kind === 'source-mailbox' ? selectedView.sourceId : null
    const closeView = () => {
      if (!viewId) {
        return
      }
      const closingViewId = viewId
      viewId = undefined
      viewIdRef.current = undefined
      runtimeSessionClient.closeView(closingViewId)
    }
    const request = buildMessagePageRequest(
      selectedView,
      preparedSearchQuery,
      sort,
      null,
      abort.signal,
      operation,
    )

    void runtimeSessionClient
      .openMessageListView(request)
      .then((opened) => {
        viewId = opened.viewId
        if (closed) {
          closeView()
          return
        }
        const { snapshot, viewId: openedViewId } = opened
        viewIdRef.current = openedViewId
        setHasMore(snapshot.data.continuation.hasAfter)
        queryClient.setQueryData(
          queryKey,
          applySnapshotToQueryData({ ...snapshot, viewId: openedViewId }),
        )
        unsubscribe = runtimeSessionClient.subscribe(
          {
            onFrame(frame) {
              switch (frame.type) {
                case 'viewSnapshot':
                case 'viewReplace':
                  if (frame.viewId !== openedViewId) {
                    return
                  }
                  uiLogger.debug(
                    {
                      event: LOG_EVENTS.viewSnapshotApplied,
                      viewId: frame.viewId,
                      type: frame.type,
                      sessionSeq: frame.sessionSeq,
                      revision: frame.revision,
                      rowCount: frame.snapshot.data.rows.length,
                      flaggedCount: frame.snapshot.data.rows.filter(
                        (row) => row.projection.isFlagged,
                      ).length,
                    },
                    'mail-list view frame applied',
                  )
                  setHasMore(frame.snapshot.data.continuation.hasAfter)
                  queryClient.setQueryData(
                    queryKey,
                    applySnapshotToQueryData(frame.snapshot),
                  )
                  return
                case 'viewDelta':
                  // Row-local change: reconcile the delta in place. Structural
                  // changes (window/coverage) still arrive as viewReplace, so
                  // `hasMore` is unchanged here (replication client-link).
                  if (frame.viewId !== openedViewId) {
                    return
                  }
                  uiLogger.debug(
                    {
                      event: LOG_EVENTS.viewDeltaApplied,
                      viewId: frame.viewId,
                      type: frame.type,
                      sessionSeq: frame.sessionSeq,
                      revision: frame.revision,
                      upsertCount: frame.delta.upserts.length,
                      orderChanged: frame.delta.order !== null,
                    },
                    'mail-list delta applied',
                  )
                  queryClient.setQueryData<
                    InfiniteData<MessagePage, string | null>
                  >(queryKey, (current) =>
                    applyDeltaToQueryData(current, frame.delta),
                  )
                  return
                case 'viewError':
                case 'viewClosed':
                case 'mutationNotification':
                case 'notification':
                case 'heartbeat':
                  return
              }
            },
          },
          { afterSeq: 0, sourceId },
        )
      })
      .catch((cause: unknown) => {
        if (closed) {
          return
        }
        const openError =
          cause instanceof Error ? cause : new Error(String(cause))
        uiLogger.error(
          {
            event: LOG_EVENTS.viewOpenFailed,
            operationId: operation.operationId,
            operationKind: operation.operationKind,
            sourceId: sourceId ?? undefined,
            error: openError.message,
          },
          'mail-list view open failed',
        )
        // Surface the failure so the renderer can show an inline error + retry
        // instead of an infinite loading skeleton (avoid broad invalidation so
        // this path stays targeted).
        setError(openError)
      })

    return () => {
      closed = true
      abort.abort()
      unsubscribe?.()
      closeView()
      setHasMore(false)
      // The effect re-runs on retry/view change; clear any prior fatal error in
      // cleanup (runs before the next open) so the skeleton can show again while
      // the new open is in flight.
      setError(null)
    }
  }, [
    enabled,
    operation,
    preparedSearchQuery,
    queryClient,
    queryKey,
    retryNonce,
    selectedView,
    sort,
  ])

  const loadMore = useCallback(() => {
    const viewId = viewIdRef.current
    if (!viewId || loadingMoreRef.current || !hasMore) {
      return
    }
    loadingMoreRef.current = true
    setIsLoadingMore(true)
    void runtimeSessionClient
      .extendMessageListView(viewId, MESSAGE_PAGE_SIZE)
      .then((result) => {
        // The view id is unchanged; the broadcast viewReplace also lands, but
        // applying the returned snapshot here keeps the scroll responsive.
        if (viewIdRef.current !== viewId) {
          return
        }
        setHasMore(result.snapshot.data.continuation.hasAfter)
        queryClient.setQueryData(
          queryKey,
          applySnapshotToQueryData(result.snapshot),
        )
      })
      .catch((cause: unknown) => {
        // A transient loadMore failure leaves the open view intact (it can be
        // retried by scrolling again), but it must not be silent.
        const extendError =
          cause instanceof Error ? cause : new Error(String(cause))
        uiLogger.error(
          {
            event: LOG_EVENTS.viewExtendFailed,
            viewId,
            error: extendError.message,
          },
          'mail-list view extend failed',
        )
      })
      .finally(() => {
        loadingMoreRef.current = false
        setIsLoadingMore(false)
      })
  }, [hasMore, queryClient, queryKey])

  return { items, isLoading, error, retry, hasMore, isLoadingMore, loadMore }
}
