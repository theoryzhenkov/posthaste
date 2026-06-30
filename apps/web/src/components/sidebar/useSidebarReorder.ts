/**
 * Drag-to-reorder persistence for the sidebar (and settings) lists. The new
 * order is written to `AppSettings` (smart_mailbox_order / account_order) via the
 * settings patch — the single source of truth the backend re-resolves from. The
 * matching list query is updated optimistically for instant feedback, then
 * invalidated to reconcile (which also self-corrects on a failed patch).
 *
 * @spec docs/L1-accounts#sidebar-ordering
 */
import { useQueryClient, type QueryKey } from '@tanstack/react-query'
import { useCallback } from 'react'

import type { AccountOverview, SmartMailboxSummary } from '@/api/types'
import { queryKeys } from '@/queryKeys'
import { runtimeMutations } from '@/runtime/mutations'

function reorderById<T extends { id: string }>(
  items: T[],
  orderedIds: string[],
): T[] {
  const byId = new Map(items.map((item) => [item.id, item]))
  const ordered = orderedIds.flatMap((id) => {
    const item = byId.get(id)
    return item ? [item] : []
  })
  // Keep any items the new order omitted (defensive against drift).
  const seen = new Set(orderedIds)
  return [...ordered, ...items.filter((item) => !seen.has(item.id))]
}

export function useSidebarReorder() {
  const queryClient = useQueryClient()

  const persist = useCallback(
    (
      input: { smartMailboxOrder?: string[]; accountOrder?: string[] },
      reconcileKeys: QueryKey[],
    ) => {
      void runtimeMutations.settings
        .patch(input)
        .finally(() =>
          reconcileKeys.forEach((queryKey) =>
            queryClient.invalidateQueries({ queryKey }),
          ),
        )
    },
    [queryClient],
  )

  const reorderSmartMailboxes = useCallback(
    (orderedIds: string[]) => {
      queryClient.setQueryData<SmartMailboxSummary[]>(
        queryKeys.smartMailboxes,
        (prev) => (prev ? reorderById(prev, orderedIds) : prev),
      )
      persist({ smartMailboxOrder: orderedIds }, [
        queryKeys.smartMailboxes,
        queryKeys.settings,
      ])
    },
    [persist, queryClient],
  )

  const reorderAccounts = useCallback(
    (orderedIds: string[]) => {
      queryClient.setQueryData<AccountOverview[]>(queryKeys.accounts, (prev) =>
        prev ? reorderById(prev, orderedIds) : prev,
      )
      persist({ accountOrder: orderedIds }, [
        queryKeys.accounts,
        queryKeys.settings,
        queryKeys.mailNavigationRead,
      ])
    },
    [persist, queryClient],
  )

  return { reorderSmartMailboxes, reorderAccounts }
}
