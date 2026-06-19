import { useEffect } from 'react'

import { useQueryClient, type InfiniteData } from '@tanstack/react-query'

import type { MessagePage } from '@/api/types'
import { runtimeSubscriptions } from '@/runtime/subscriptions'
import type {
  RuntimeMailListViewState,
  RuntimeViewSnapshot,
} from '@/runtime/types'
import { runtimeViews } from '@/runtime/views'
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
    let unsubscribe: (() => void) | undefined
    const abort = new AbortController()
    const request = buildMessagePageRequest(
      selectedView,
      preparedSearchQuery,
      sort,
      null,
      abort.signal,
      operation,
    )

    void runtimeViews.mail
      .openMessageList(request)
      .then(({ snapshot, viewId }) => {
        if (closed) return
        queryClient.setQueryData(
          queryKey,
          applySnapshotToQueryData({ ...snapshot, viewId }),
        )
        unsubscribe = runtimeSubscriptions.view(
          {
            viewId,
            afterRevision: snapshot.revision,
            sourceId:
              selectedView.kind === 'source-mailbox'
                ? selectedView.sourceId
                : null,
          },
          {
            onFrame(frame) {
              if (frame.kind !== 'snapshot' && frame.kind !== 'replace') {
                return
              }
              queryClient.setQueryData(
                queryKey,
                applySnapshotToQueryData(frame.snapshot),
              )
            },
          },
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
