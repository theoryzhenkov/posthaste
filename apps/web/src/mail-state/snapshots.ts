import type { QueryClient, QueryKey } from '@tanstack/react-query'

import type { QuerySnapshot } from './types'

export function snapshotQuery(
  queryClient: QueryClient,
  queryKey: QueryKey,
): QuerySnapshot {
  const state = queryClient.getQueryState(queryKey)
  return {
    data: queryClient.getQueryData(queryKey),
    existed: state !== undefined,
    queryKey,
  }
}
