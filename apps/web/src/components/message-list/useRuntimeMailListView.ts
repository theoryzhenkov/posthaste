import { useEffect } from 'react'

import { useQueryClient, type InfiniteData } from '@tanstack/react-query'

import type { MessagePage } from '@/api/types'
import { runtimeStream } from '@/runtime/runtimeStream'
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
    let sessionId: string | undefined
    let unsubscribe: (() => void) | undefined
    const abort = new AbortController()
    const sourceId =
      selectedView.kind === 'source-mailbox' ? selectedView.sourceId : null
    const closeSession = () => {
      if (!sessionId) {
        return
      }
      const closingSessionId = sessionId
      sessionId = undefined
      void runtimeStream
        .closeSession(closingSessionId, sourceId)
        .catch(() => {})
    }
    const request = buildMessagePageRequest(
      selectedView,
      preparedSearchQuery,
      sort,
      null,
      abort.signal,
      operation,
    )

    void runtimeStream
      .openSession({ sourceId })
      .then((session) =>
        runtimeStream
          .openMessageListView({ sessionId: session.sessionId, view: request })
          .then(({ snapshot, viewId }) => ({
            sessionId: session.sessionId,
            snapshot,
            viewId,
          })),
      )
      .then((opened) => {
        sessionId = opened.sessionId
        if (closed) {
          closeSession()
          return
        }
        const { snapshot, viewId } = opened
        queryClient.setQueryData(
          queryKey,
          applySnapshotToQueryData({ ...snapshot, viewId }),
        )
        unsubscribe = runtimeStream.subscribe(
          { sessionId, afterSeq: 0, sourceId },
          {
            onFrame(frame) {
              switch (frame.type) {
                case 'viewSnapshot':
                case 'viewReplace':
                  if (frame.viewId !== viewId) {
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
                case 'heartbeat':
                  return
              }
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
      closeSession()
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
