import { useEffect } from 'react'

import { useQueryClient, type InfiniteData } from '@tanstack/react-query'

import type { MessagePage } from '@/api/types'
import { runtimeSessionClient } from '@/runtime/sessionClient'
import type {
  RuntimeMailListViewState,
  RuntimeViewSnapshot,
} from '@/runtime/types'
import type { OperationContext } from '@/observability'
import type { PreparedServerSearchQuery } from '@/searchQuery'
import type { SidebarSelection } from '../Sidebar'
import type { SortConfig } from '../thread-list/columns'
import { buildMessagePageRequest } from './model'

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
}) {
  const queryClient = useQueryClient()

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
                  queryClient.setQueryData(
                    queryKey,
                    applySnapshotToQueryData(frame.snapshot),
                  )
                  return
                case 'viewError':
                case 'viewClosed':
                case 'mutationSettlement':
                case 'notification':
                case 'mutationHistory':
                case 'heartbeat':
                  return
              }
            },
          },
          { afterSeq: 0, sourceId },
        )
      })
      .catch(() => {
        // The legacy query path remains available by disabling the feature flag;
        // avoid broad invalidation/refetch here so this path stays targeted.
      })

    return () => {
      closed = true
      abort.abort()
      unsubscribe?.()
      closeView()
    }
  }, [
    enabled,
    operation,
    preparedSearchQuery,
    queryClient,
    queryKey,
    selectedView,
    sort,
  ])
}
