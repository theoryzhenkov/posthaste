// Cursor-paged live mail list. The backend answers one screenful at a time
// (rows + an opaque continuation cursor); this hook mounts one live query per
// fetched page — the base scope, then the scope at each accumulated cursor —
// so every page keeps refetching on the event stream like any other mounted
// query. Rows are concatenated (deduplicated by id, since a refetch can shift
// page boundaries) and `loadMore` appends the last page's cursor.
//
// Only the cursor list is component state; every row rendered comes from the
// facade's mirror.

import { useCallback, useMemo, useRef, useState, useSyncExternalStore } from 'react'
import { canonicalQueryKey, type LiveResult, type QueryStatus } from '../client'
import { useMailClient } from '../hooks'
import type { MailListQuery, MailListResult, MessageSummary, Query } from '../gen'

export interface MailListPages {
  rows: MessageSummary[]
  /** The first page's status: 'loading' only before the first answer. */
  status: QueryStatus
  error: Error | null
  /** Whether another page exists (the last page returned a cursor). */
  hasMore: boolean
  /** Whether a further page is currently being fetched. */
  loadingMore: boolean
  loadMore: () => void
}

interface PagesSnapshot {
  rows: MessageSummary[]
  status: QueryStatus
  error: Error | null
  hasMore: boolean
  loadingMore: boolean
  nextCursor: string | null
}

const PAGE_SIZE = 50

export function useMailListPages(scope: MailListQuery): MailListPages {
  const client = useMailClient()
  const baseQuery: Query = useMemo(
    () => ({ mailList: { ...scope, limit: PAGE_SIZE } }),
    [scope],
  )
  const scopeKey = canonicalQueryKey(baseQuery)

  // Cursors of the pages after the first, tagged with the scope they belong
  // to; switching views implicitly resets to a single page.
  const [paging, setPaging] = useState<{ scopeKey: string; cursors: string[] }>({
    scopeKey,
    cursors: [],
  })
  const cursors = paging.scopeKey === scopeKey ? paging.cursors : []

  const queries: Query[] = useMemo(
    () => [
      baseQuery,
      ...cursors.map((cursor): Query => ({ mailList: { ...scope, limit: PAGE_SIZE, cursor } })),
    ],
    [scopeKey, cursors],
  )
  const keys = useMemo(() => queries.map(canonicalQueryKey), [queries])
  const keysKey = keys.join('\n')

  const subscribe = useCallback(
    (onChange: () => void) => {
      const retained = queries.map((q) => client.retain(q))
      const unsubscribes = retained.map((k) => client.subscribeQuery(k, onChange))
      return () => {
        for (const u of unsubscribes) u()
        for (const k of retained) client.release(k)
      }
    },
    [client, keysKey],
  )

  // getSnapshot must return the same object until an underlying page snapshot
  // changes, so the concatenation is cached against the page snapshots.
  const cache = useRef<{ parts: LiveResult<MailListResult>[]; value: PagesSnapshot } | null>(null)
  const getSnapshot = useCallback((): PagesSnapshot => {
    const parts = keys.map((k) => client.getSnapshot<MailListResult>(k))
    const cached = cache.current
    if (
      cached &&
      cached.parts.length === parts.length &&
      cached.parts.every((p, i) => p === parts[i])
    ) {
      return cached.value
    }
    const seen = new Set<string>()
    const rows: MessageSummary[] = []
    for (const part of parts) {
      for (const row of part.data?.rows ?? []) {
        if (!seen.has(row.id)) {
          seen.add(row.id)
          rows.push(row)
        }
      }
    }
    const first = parts[0]!
    const last = parts[parts.length - 1]!
    const value: PagesSnapshot = {
      rows,
      status: first.status,
      error: first.error,
      hasMore: last.data?.nextCursor != null,
      loadingMore: parts.length > 1 && last.status === 'loading',
      nextCursor: last.data?.nextCursor ?? null,
    }
    cache.current = { parts, value }
    return value
  }, [client, keysKey])

  const snapshot = useSyncExternalStore(subscribe, getSnapshot)

  const loadMore = useCallback(() => {
    const { nextCursor } = getSnapshot()
    if (!nextCursor) return
    setPaging((prev) => {
      const current = prev.scopeKey === scopeKey ? prev.cursors : []
      if (current.includes(nextCursor)) return prev
      return { scopeKey, cursors: [...current, nextCursor] }
    })
  }, [getSnapshot, scopeKey])

  return {
    rows: snapshot.rows,
    status: snapshot.status,
    error: snapshot.error,
    hasMore: snapshot.hasMore,
    loadingMore: snapshot.loadingMore,
    loadMore,
  }
}
