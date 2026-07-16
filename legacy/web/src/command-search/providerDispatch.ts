import type { Dispatch, SetStateAction } from 'react'

import { LOG_EVENTS } from '@/logEvents'
import { uiLogger } from '@/logger'

import type { ProviderState, RankingContext, SearchProvider } from './types'
import { cloneStatesWith, providerLimit, queryShape } from './searchState'

type MutableRef<T> = { current: T }

export function dispatchProviderSearch(input: {
  append: boolean
  context: RankingContext
  cursor?: string
  provider: SearchProvider
  query: string
  queryVersionRef: MutableRef<number>
  setProviderStates: Dispatch<SetStateAction<Map<string, ProviderState>>>
  setStaleSearchCount: Dispatch<SetStateAction<number>>
  signal: AbortSignal
  version: number
}) {
  const {
    append,
    context,
    cursor,
    provider,
    query,
    queryVersionRef,
    setProviderStates,
    setStaleSearchCount,
    signal,
    version,
  } = input
  const providerId = provider.id
  const startedAt = performance.now()
  return provider
    .search({
      query,
      cursor,
      limit: providerLimit(provider.id),
      context,
      signal,
    })
    .then((page) => {
      if (queryVersionRef.current !== version) {
        uiLogger.debug(
          {
            event: LOG_EVENTS.paletteSearchStale,
            providerId,
            ...queryShape(query),
          },
          `stale command palette provider ${append ? 'page ' : ''}response ignored`,
        )
        setStaleSearchCount((count) => count + 1)
        return
      }
      if (signal.aborted) return

      const latencyMs =
        page.latencyMs ?? Math.round(performance.now() - startedAt)
      uiLogger.debug(
        {
          event: LOG_EVENTS.paletteProviderCompleted,
          providerId,
          candidateCount: page.candidates.length,
          hasNextCursor: page.nextCursor !== null,
          latencyMs,
          ...queryShape(query),
        },
        `command palette provider ${append ? 'page ' : ''}completed`,
      )
      setProviderStates((states) =>
        cloneStatesWith(states, providerId, (state) => ({
          status: 'done',
          candidates: append
            ? [...state.candidates, ...page.candidates]
            : page.candidates,
          nextCursor: page.nextCursor,
          latencyMs,
          indexVersion: page.indexVersion,
        })),
      )
    })
    .catch((error: unknown) => {
      if (queryVersionRef.current !== version) {
        uiLogger.debug(
          {
            event: LOG_EVENTS.paletteSearchStale,
            providerId,
            ...queryShape(query),
          },
          `stale command palette provider ${append ? 'page ' : ''}error ignored`,
        )
        setStaleSearchCount((count) => count + 1)
        return
      }
      if (signal.aborted) return
      const latencyMs = Math.round(performance.now() - startedAt)
      uiLogger.warn(
        {
          event: LOG_EVENTS.paletteProviderFailed,
          providerId,
          latencyMs,
          ...queryShape(query),
        },
        `command palette provider ${append ? 'page ' : ''}failed`,
      )
      setProviderStates((states) =>
        cloneStatesWith(states, providerId, (state) => ({
          ...state,
          status: 'error',
          error,
          latencyMs,
        })),
      )
    })
}
