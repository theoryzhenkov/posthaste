import {
  useCallback,
  useEffect,
  type Dispatch,
  type SetStateAction,
} from 'react'

import { LOG_EVENTS } from '@/logEvents'
import { uiLogger } from '@/logger'

import { dispatchProviderSearch } from './providerDispatch'
import {
  emptyProviderState,
  initialProviderStates,
  queryShape,
  REMOTE_DEBOUNCE_MS,
  SETTLEMENT_HARD_CAP_MS,
} from './searchState'
import type { ProviderState, RankingContext, SearchProvider } from './types'

type MutableRef<T> = { current: T }

export function useSearchLifecycle(input: {
  abortControllerRef: MutableRef<AbortController | null>
  bestMatchIdsRef: MutableRef<string[]>
  context: RankingContext
  lastQueryRef: MutableRef<string | null>
  loadMoreControllersRef: MutableRef<AbortController[]>
  providers: SearchProvider[]
  query: string
  queryVersionRef: MutableRef<number>
  setCancelledSearchCount: Dispatch<SetStateAction<number>>
  setFrozenBestMatchIds: Dispatch<SetStateAction<string[] | null>>
  setIsSettled: Dispatch<SetStateAction<boolean>>
  setProviderStates: Dispatch<SetStateAction<Map<string, ProviderState>>>
  setQueryVersion: Dispatch<SetStateAction<number>>
  setSelectedCandidateId: Dispatch<SetStateAction<string | null>>
  setStaleSearchCount: Dispatch<SetStateAction<number>>
}) {
  const {
    abortControllerRef,
    bestMatchIdsRef,
    context,
    lastQueryRef,
    loadMoreControllersRef,
    providers,
    query,
    queryVersionRef,
    setCancelledSearchCount,
    setFrozenBestMatchIds,
    setIsSettled,
    setProviderStates,
    setQueryVersion,
    setSelectedCandidateId,
    setStaleSearchCount,
  } = input

  const freezeWith = useCallback(
    (ids: string[]) => {
      setFrozenBestMatchIds((current) => current ?? ids)
      setIsSettled(true)
    },
    [setFrozenBestMatchIds, setIsSettled],
  )

  const abortCurrentRequests = useCallback(() => {
    abortControllerRef.current?.abort()
    for (const controller of loadMoreControllersRef.current) {
      controller.abort()
    }
    loadMoreControllersRef.current = []
  }, [abortControllerRef, loadMoreControllersRef])

  const cancel = useCallback(() => {
    abortCurrentRequests()
    uiLogger.debug(
      {
        event: LOG_EVENTS.paletteSearchCancelled,
        ...queryShape(query),
      },
      'command palette search cancelled',
    )
    setCancelledSearchCount((count) => count + 1)
  }, [abortCurrentRequests, query, setCancelledSearchCount])

  useEffect(() => {
    abortCurrentRequests()
    const controller = new AbortController()
    abortControllerRef.current = controller
    const nextVersion = queryVersionRef.current + 1
    queryVersionRef.current = nextVersion
    setQueryVersion(nextVersion)

    const queryChanged = lastQueryRef.current !== query
    lastQueryRef.current = query
    if (queryChanged) {
      setSelectedCandidateId(null)
      setFrozenBestMatchIds(null)
      setIsSettled(false)
      setProviderStates(initialProviderStates(providers))
    } else {
      setProviderStates((prev) => {
        const next = new Map<string, ProviderState>()
        for (const provider of providers) {
          next.set(provider.id, {
            ...(prev.get(provider.id) ?? emptyProviderState()),
            status: 'loading',
          })
        }
        return next
      })
    }

    const dispatch = (provider: SearchProvider) => {
      void dispatchProviderSearch({
        append: false,
        context,
        provider,
        query,
        queryVersionRef,
        setProviderStates,
        setStaleSearchCount,
        signal: controller.signal,
        version: nextVersion,
      })
    }

    for (const provider of providers) {
      if (!provider.remote) dispatch(provider)
    }

    const remoteProviders = providers.filter((provider) => provider.remote)
    let remoteTimer: number | undefined
    if (remoteProviders.length > 0) {
      if (query.trim()) {
        remoteTimer = window.setTimeout(() => {
          if (
            queryVersionRef.current !== nextVersion ||
            controller.signal.aborted
          ) {
            return
          }
          for (const provider of remoteProviders) dispatch(provider)
        }, REMOTE_DEBOUNCE_MS)
      } else {
        for (const provider of remoteProviders) dispatch(provider)
      }
    }

    const hardCapTimer = window.setTimeout(() => {
      if (queryVersionRef.current === nextVersion) {
        freezeWith(bestMatchIdsRef.current)
      }
    }, SETTLEMENT_HARD_CAP_MS)

    return () => {
      if (remoteTimer !== undefined) window.clearTimeout(remoteTimer)
      window.clearTimeout(hardCapTimer)
      controller.abort()
    }
  }, [
    abortControllerRef,
    abortCurrentRequests,
    bestMatchIdsRef,
    context,
    freezeWith,
    lastQueryRef,
    providers,
    query,
    queryVersionRef,
    setFrozenBestMatchIds,
    setIsSettled,
    setProviderStates,
    setQueryVersion,
    setSelectedCandidateId,
    setStaleSearchCount,
  ])

  return { cancel, freezeWith }
}
