import { useCallback, useEffect, useMemo, useRef, useState } from 'react'

import { LOG_EVENTS } from '@/logEvents'
import { uiLogger } from '@/logger'

import { buildPaletteRows } from './ranker'
import type {
  CommandSearchController,
  CommandSearchSession,
  ProviderState,
  RankingContext,
  SearchProvider,
} from './types'

// Remote (backend) provider dispatch is debounced by this much while the user
// is still typing, so each keystroke does not issue a backend request.
const REMOTE_DEBOUNCE_MS = 180
// Safety net only: Best matches normally freezes when every provider settles
// (see the settlement effect). This caps the wait if a request never resolves.
const SETTLEMENT_HARD_CAP_MS = 2000

const PROVIDER_LIMITS: Record<string, number> = {
  commands: 20,
  'query-completions': 12,
  mailboxes: 16,
  tags: 12,
  messages: 12,
}

function emptyProviderState(): ProviderState {
  return {
    status: 'idle',
    candidates: [],
    nextCursor: null,
  }
}

function initialProviderStates(
  providers: SearchProvider[],
): Map<string, ProviderState> {
  return new Map(
    providers.map((provider) => [
      provider.id,
      {
        ...emptyProviderState(),
        status: 'loading' as const,
      },
    ]),
  )
}

function providerLimit(providerId: string): number {
  return PROVIDER_LIMITS[providerId] ?? 8
}

function cloneStatesWith(
  states: Map<string, ProviderState>,
  providerId: string,
  update: (state: ProviderState) => ProviderState,
): Map<string, ProviderState> {
  const next = new Map(states)
  next.set(providerId, update(next.get(providerId) ?? emptyProviderState()))
  return next
}

function allProvidersSettled(states: Map<string, ProviderState>): boolean {
  return [...states.values()].every((state) => state.status !== 'loading')
}

function queryShape(query: string) {
  return {
    queryLength: query.length,
    queryTokenCount: query.trim() ? query.trim().split(/\s+/).length : 0,
  }
}

export function useCommandSearch(input: {
  query: string
  context: RankingContext
  providers: SearchProvider[]
}): CommandSearchController {
  const { query, context, providers } = input
  const [queryVersion, setQueryVersion] = useState(0)
  const [providerStates, setProviderStates] = useState<
    Map<string, ProviderState>
  >(() => initialProviderStates(providers))
  const [selectedCandidateId, setSelectedCandidateId] = useState<string | null>(
    null,
  )
  const [frozenBestMatchIds, setFrozenBestMatchIds] = useState<string[] | null>(
    null,
  )
  const [isSettled, setIsSettled] = useState(false)
  const [cancelledSearchCount, setCancelledSearchCount] = useState(0)
  const [staleSearchCount, setStaleSearchCount] = useState(0)
  const abortControllerRef = useRef<AbortController | null>(null)
  const loadMoreControllersRef = useRef<AbortController[]>([])
  const queryVersionRef = useRef(0)
  const providersRef = useRef(providers)
  const statesRef = useRef(providerStates)
  const bestMatchIdsRef = useRef<string[]>([])
  const lastQueryRef = useRef<string | null>(null)

  useEffect(() => {
    providersRef.current = providers
  }, [providers])

  useEffect(() => {
    statesRef.current = providerStates
  }, [providerStates])

  const rowResult = useMemo(
    () =>
      buildPaletteRows({
        query,
        context,
        providerStates,
        frozenBestMatchIds,
      }),
    [context, frozenBestMatchIds, providerStates, query],
  )
  useEffect(() => {
    bestMatchIdsRef.current = rowResult.bestMatchIds
  }, [rowResult.bestMatchIds])

  const freezeWith = useCallback((ids: string[]) => {
    setFrozenBestMatchIds((current) => current ?? ids)
    setIsSettled(true)
  }, [])

  // Freeze Best matches deterministically once every selected provider has
  // settled (returned or errored), using the best-match IDs from this settled
  // render. Waiting for settlement keeps the slower message provider in Best
  // matches instead of locking it out on a fixed timer. The freeze is deferred
  // to a task so it does not setState synchronously within the effect.
  useEffect(() => {
    if (frozenBestMatchIds !== null || !allProvidersSettled(providerStates)) {
      return
    }
    const ids = rowResult.bestMatchIds
    const timer = window.setTimeout(() => freezeWith(ids), 0)
    return () => window.clearTimeout(timer)
  }, [freezeWith, frozenBestMatchIds, providerStates, rowResult.bestMatchIds])

  const abortCurrentRequests = useCallback(() => {
    abortControllerRef.current?.abort()
    for (const controller of loadMoreControllersRef.current) {
      controller.abort()
    }
    loadMoreControllersRef.current = []
  }, [])

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
  }, [abortCurrentRequests, query])

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
      // A new query invalidates selection and the frozen Best matches.
      setSelectedCandidateId(null)
      setFrozenBestMatchIds(null)
      setIsSettled(false)
      setProviderStates(initialProviderStates(providers))
    } else {
      // The provider set or context changed for the same query (e.g. /read
      // hydrated mailboxes while the palette was open). Re-run without dropping
      // the user's selection or the frozen Best matches; mark providers loading
      // so fresh results replace stale ones in place.
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
      const startedAt = performance.now()
      void provider
        .search({
          query,
          limit: providerLimit(provider.id),
          context,
          signal: controller.signal,
        })
        .then((page) => {
          if (queryVersionRef.current !== nextVersion) {
            uiLogger.debug(
              {
                event: LOG_EVENTS.paletteSearchStale,
                providerId: provider.id,
                ...queryShape(query),
              },
              'stale command palette provider response ignored',
            )
            setStaleSearchCount((count) => count + 1)
            return
          }
          if (controller.signal.aborted) return

          uiLogger.debug(
            {
              event: LOG_EVENTS.paletteProviderCompleted,
              providerId: provider.id,
              candidateCount: page.candidates.length,
              hasNextCursor: page.nextCursor !== null,
              latencyMs:
                page.latencyMs ?? Math.round(performance.now() - startedAt),
              ...queryShape(query),
            },
            'command palette provider completed',
          )
          setProviderStates((states) =>
            cloneStatesWith(states, provider.id, () => ({
              status: 'done',
              candidates: page.candidates,
              nextCursor: page.nextCursor,
              latencyMs:
                page.latencyMs ?? Math.round(performance.now() - startedAt),
              indexVersion: page.indexVersion,
            })),
          )
        })
        .catch((error: unknown) => {
          if (queryVersionRef.current !== nextVersion) {
            uiLogger.debug(
              {
                event: LOG_EVENTS.paletteSearchStale,
                providerId: provider.id,
                ...queryShape(query),
              },
              'stale command palette provider error ignored',
            )
            setStaleSearchCount((count) => count + 1)
            return
          }
          if (controller.signal.aborted) return
          uiLogger.warn(
            {
              event: LOG_EVENTS.paletteProviderFailed,
              providerId: provider.id,
              latencyMs: Math.round(performance.now() - startedAt),
              ...queryShape(query),
            },
            'command palette provider failed',
          )
          setProviderStates((states) =>
            cloneStatesWith(states, provider.id, (state) => ({
              ...state,
              status: 'error',
              error,
              latencyMs: Math.round(performance.now() - startedAt),
            })),
          )
        })
    }

    // Local providers are synchronous/in-memory: dispatch immediately.
    for (const provider of providers) {
      if (!provider.remote) dispatch(provider)
    }

    // Remote providers issue a backend request, so debounce their dispatch
    // while the user is typing. On an empty query they serve local recents,
    // which is not a backend call and therefore is not debounced.
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
  }, [abortCurrentRequests, context, freezeWith, providers, query])

  const loadMore = useCallback(
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

      const startedAt = performance.now()
      void provider
        .search({
          query,
          cursor: currentState.nextCursor,
          limit: providerLimit(provider.id),
          context,
          signal: controller.signal,
        })
        .then((page) => {
          if (queryVersionRef.current !== version) {
            uiLogger.debug(
              {
                event: LOG_EVENTS.paletteSearchStale,
                providerId,
                ...queryShape(query),
              },
              'stale command palette page response ignored',
            )
            setStaleSearchCount((count) => count + 1)
            return
          }
          if (controller.signal.aborted) return
          uiLogger.debug(
            {
              event: LOG_EVENTS.paletteProviderCompleted,
              providerId,
              candidateCount: page.candidates.length,
              hasNextCursor: page.nextCursor !== null,
              latencyMs:
                page.latencyMs ?? Math.round(performance.now() - startedAt),
              ...queryShape(query),
            },
            'command palette provider page completed',
          )
          setProviderStates((states) =>
            cloneStatesWith(states, providerId, (state) => ({
              status: 'done',
              candidates: [...state.candidates, ...page.candidates],
              nextCursor: page.nextCursor,
              latencyMs:
                page.latencyMs ?? Math.round(performance.now() - startedAt),
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
              'stale command palette page error ignored',
            )
            setStaleSearchCount((count) => count + 1)
            return
          }
          if (controller.signal.aborted) return
          uiLogger.warn(
            {
              event: LOG_EVENTS.paletteProviderFailed,
              providerId,
              latencyMs: Math.round(performance.now() - startedAt),
              ...queryShape(query),
            },
            'command palette provider page failed',
          )
          setProviderStates((states) =>
            cloneStatesWith(states, providerId, (state) => ({
              ...state,
              status: 'error',
              error,
              latencyMs: Math.round(performance.now() - startedAt),
            })),
          )
        })
        .finally(() => {
          loadMoreControllersRef.current =
            loadMoreControllersRef.current.filter((item) => item !== controller)
        })
    },
    [context, query],
  )

  const session: CommandSearchSession = useMemo(
    () => ({
      query,
      queryVersion,
      context,
      providerStates,
      rows: rowResult.rows,
      selectedCandidateId,
      isLoading: [...providerStates.values()].some(
        (state) => state.status === 'loading',
      ),
      isSettled,
      cancelledSearchCount,
      staleSearchCount,
    }),
    [
      cancelledSearchCount,
      context,
      isSettled,
      providerStates,
      query,
      queryVersion,
      rowResult.rows,
      selectedCandidateId,
      staleSearchCount,
    ],
  )

  return {
    session,
    loadMore,
    cancel,
    select: setSelectedCandidateId,
  }
}
