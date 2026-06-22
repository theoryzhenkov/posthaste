import type { QueryClient } from '@tanstack/react-query'

import type { AccountOverview } from '../api/types'
import { queryKeys } from '../queryKeys'
import { invalidateAccountReadModels } from './invalidations'

/**
 * Apply a config-mutation result while preserving live runtime state.
 *
 * Config and runtime are separate concerns: a config mutation (rename, enable,
 * appearance, ...) must not clobber the runtime state the event stream owns.
 * Runtime is preserved from the current cache entry; the freshly-created
 * account uses the result's runtime.
 */
function mergeConfigPreserveRuntime(
  current: AccountOverview | undefined,
  next: AccountOverview,
): AccountOverview {
  return current ? { ...next, runtime: current.runtime } : next
}

export function mergeAccountOverview(
  queryClient: QueryClient,
  account: AccountOverview,
) {
  queryClient.setQueryData<AccountOverview[]>(
    queryKeys.accounts,
    (current = []) => {
      const index = current.findIndex(
        (candidate) => candidate.id === account.id,
      )
      if (index === -1) {
        return [...current, account]
      }
      return current.map((candidate) =>
        candidate.id === account.id
          ? mergeConfigPreserveRuntime(candidate, account)
          : candidate,
      )
    },
  )
  queryClient.setQueryData<AccountOverview>(
    queryKeys.account(account.id),
    (current) => mergeConfigPreserveRuntime(current, account),
  )
}

export function removeAccountOverview(
  queryClient: QueryClient,
  accountId: string,
) {
  queryClient.setQueryData<AccountOverview[]>(
    queryKeys.accounts,
    (current = []) => current.filter((account) => account.id !== accountId),
  )
  queryClient.removeQueries({
    queryKey: queryKeys.account(accountId),
    exact: true,
  })
}

export function applyAccountMutationResult(
  queryClient: QueryClient,
  account: AccountOverview,
) {
  mergeAccountOverview(queryClient, account)
  invalidateAccountReadModels(queryClient, account.id)
}
