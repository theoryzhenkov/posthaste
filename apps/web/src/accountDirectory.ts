/**
 * Account directory selectors backed by the canonical accounts query.
 *
 * Mutable account display fields should be rendered from `sourceId` lookups
 * instead of stale snapshots embedded in message or conversation DTOs.
 *
 * @spec docs/L1-ui#data-fetching
 */
import { useMemo } from 'react'
import { useQuery } from '@tanstack/react-query'
import { fetchAccounts } from './api/client'
import type { AccountOverview, MessageSummary } from './api/types'
import { queryKeys } from './queryKeys'

export interface AccountDirectory {
  accounts: AccountOverview[]
  byId: ReadonlyMap<string, AccountOverview>
  resolveAccountName: (sourceId: string, fallback?: string | null) => string
}

function buildAccountMap(accounts: AccountOverview[]) {
  return new Map(accounts.map((account) => [account.id, account]))
}

export function createAccountDirectory(
  accounts: AccountOverview[],
): AccountDirectory {
  const byId = buildAccountMap(accounts)
  return {
    accounts,
    byId,
    resolveAccountName: (sourceId, fallback) =>
      byId.get(sourceId)?.name ?? fallback ?? sourceId,
  }
}

export function useAccountDirectory(): AccountDirectory {
  const { data: accounts = [] } = useQuery({
    queryKey: queryKeys.accounts,
    queryFn: fetchAccounts,
  })

  return useMemo(() => createAccountDirectory(accounts), [accounts])
}

export function applyAccountNamesToMessages(
  messages: MessageSummary[],
  directory: AccountDirectory,
): MessageSummary[] {
  return messages.map((message) => {
    const sourceName = directory.resolveAccountName(
      message.sourceId,
      message.sourceName,
    )
    return sourceName === message.sourceName
      ? message
      : { ...message, sourceName }
  })
}
