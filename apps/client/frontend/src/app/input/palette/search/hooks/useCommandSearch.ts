import { useEffect, useMemo, useRef, useState } from 'react'

import { buildPaletteRows } from '../ranker'
import { allProvidersSettled, initialProviderStates } from '../searchState'
import type {
  CommandSearchController,
  CommandSearchSession,
  ProviderState,
  RankingContext,
  SearchProvider,
} from '../types'
import { useProviderLoadMore } from './useProviderLoadMore'
import { useSearchLifecycle } from './useSearchLifecycle'

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

  const { cancel, freezeWith } = useSearchLifecycle({
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
  })

  // Freeze Best matches deterministically once every selected provider has
  // settled (returned or errored), using the best-match IDs from this settled
  // render. Waiting for settlement keeps the slower message provider in Best
  // matches instead of locking it out on a fixed timer.
  useEffect(() => {
    if (frozenBestMatchIds !== null || !allProvidersSettled(providerStates)) {
      return
    }
    const ids = rowResult.bestMatchIds
    const timer = window.setTimeout(() => freezeWith(ids), 0)
    return () => window.clearTimeout(timer)
  }, [freezeWith, frozenBestMatchIds, providerStates, rowResult.bestMatchIds])

  const loadMore = useProviderLoadMore({
    context,
    loadMoreControllersRef,
    providersRef,
    query,
    queryVersionRef,
    setProviderStates,
    setStaleSearchCount,
    statesRef,
  })

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
