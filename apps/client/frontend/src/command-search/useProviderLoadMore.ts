import { useCallback, type Dispatch, type SetStateAction } from 'react'

import { dispatchProviderSearch } from './providerDispatch'
import { cloneStatesWith } from './searchState'
import type { ProviderState, RankingContext, SearchProvider } from './types'

type MutableRef<T> = { current: T }

export function useProviderLoadMore(input: {
  context: RankingContext
  loadMoreControllersRef: MutableRef<AbortController[]>
  providersRef: MutableRef<SearchProvider[]>
  query: string
  queryVersionRef: MutableRef<number>
  setProviderStates: Dispatch<SetStateAction<Map<string, ProviderState>>>
  setStaleSearchCount: Dispatch<SetStateAction<number>>
  statesRef: MutableRef<Map<string, ProviderState>>
}) {
  const {
    context,
    loadMoreControllersRef,
    providersRef,
    query,
    queryVersionRef,
    setProviderStates,
    setStaleSearchCount,
    statesRef,
  } = input

  return useCallback(
    (providerId: string) => {
      const provider = providersRef.current.find(
        (item) => item.id === providerId,
      )
      const currentState = statesRef.current.get(providerId)
      if (
        !provider ||
        !currentState?.nextCursor ||
        currentState.status === 'loading'
      ) {
        return
      }

      const controller = new AbortController()
      loadMoreControllersRef.current.push(controller)
      const version = queryVersionRef.current
      setProviderStates((states) =>
        cloneStatesWith(states, providerId, (state) => ({
          ...state,
          status: 'loading',
        })),
      )

      void dispatchProviderSearch({
        append: true,
        context,
        cursor: currentState.nextCursor,
        provider,
        query,
        queryVersionRef,
        setProviderStates,
        setStaleSearchCount,
        signal: controller.signal,
        version,
      }).finally(() => {
        loadMoreControllersRef.current = loadMoreControllersRef.current.filter(
          (item) => item !== controller,
        )
      })
    },
    [
      context,
      loadMoreControllersRef,
      providersRef,
      query,
      queryVersionRef,
      setProviderStates,
      setStaleSearchCount,
      statesRef,
    ],
  )
}
