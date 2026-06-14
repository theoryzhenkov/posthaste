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

/** Restore previously snapshotted query entries (used for optimistic rollback). */
export function restoreSnapshots(
  queryClient: QueryClient,
  snapshots: QuerySnapshot[],
) {
  for (const snapshot of snapshots) {
    if (snapshot.existed) {
      queryClient.setQueryData(snapshot.queryKey, snapshot.data)
      continue
    }
    queryClient.removeQueries({ queryKey: snapshot.queryKey, exact: true })
  }
}
