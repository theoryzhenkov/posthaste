import { useEffect } from 'react'

import { useQueryClient } from '@tanstack/react-query'

import { runtimeSessionClient } from '@/runtime/sessionClient'
import type { RuntimeViewSnapshot } from '@/runtime/types'

/**
 * Open a single-object runtime view (e.g. `messageDetail`, `conversation`) and
 * keep the given query-cache entry fed from its snapshot and `viewReplace`
 * frames, so the surface reflects the runtime's overlay-folded (optimistic)
 * state without a renderer-side cache patch.
 *
 * The non-paged sibling of {@link useRuntimeMailListView}. Unlike the mail-list
 * path, the legacy HTTP query for `queryKey` stays enabled as the initial load
 * and (for detail) the provider body fetch; this hook layers runtime-served
 * updates on top via `merge`. `merge` defaults to replacing the cached value.
 *
 * @spec docs/runtime/adapter/L2#view-operation-flow
 */
export function useRuntimeObjectView<TData>({
  enabled,
  family,
  merge,
  payload,
  queryKey,
  sourceId,
}: {
  enabled: boolean
  family: string
  merge?: (previous: TData | undefined, next: TData) => TData
  payload: unknown
  queryKey: readonly unknown[]
  sourceId: string | null
}) {
  const queryClient = useQueryClient()
  // The payload identifies the target object; serialize it so the effect
  // reopens the view when the target changes but not on unrelated re-renders.
  const payloadKey = JSON.stringify(payload)

  useEffect(() => {
    if (!enabled) {
      return
    }

    let closed = false
    let viewId: string | undefined
    let unsubscribe: (() => void) | undefined
    const closeView = () => {
      if (!viewId) {
        return
      }
      const closingViewId = viewId
      viewId = undefined
      runtimeSessionClient.closeView(closingViewId)
    }

    const write = (data: TData) => {
      queryClient.setQueryData<TData>(queryKey, (previous) =>
        merge ? merge(previous, data) : data,
      )
    }

    void runtimeSessionClient
      .openView<TData>({ family, payload, sourceId })
      .then((opened) => {
        viewId = opened.viewId
        if (closed) {
          closeView()
          return
        }
        const openedViewId = opened.viewId
        write(opened.snapshot.data)
        unsubscribe = runtimeSessionClient.subscribe(
          {
            onFrame(frame) {
              switch (frame.type) {
                case 'viewSnapshot':
                case 'viewReplace':
                  if (frame.viewId !== openedViewId) {
                    return
                  }
                  write((frame.snapshot as RuntimeViewSnapshot<TData>).data)
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
      .catch(() => {
        // The legacy HTTP query for queryKey remains the fallback; stay quiet
        // so this path degrades to the pre-runtime-view behaviour.
      })

    return () => {
      closed = true
      unsubscribe?.()
      closeView()
    }
    // `payloadKey` stands in for `payload`, and `queryKey` is derived from the
    // same target, so the primitive deps fully capture the view identity.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [enabled, family, merge, payloadKey, queryClient, sourceId])
}
