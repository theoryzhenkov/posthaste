/**
 * Account directory selectors backed by the `accounts` query family.
 *
 * Mutable account display fields should be rendered from `sourceId` lookups
 * instead of stale snapshots embedded in message or conversation DTOs.
 */
import { useMemo } from 'react'
import type { AccountRow, MessageSummary } from '@/gen'
import { useAccounts } from '@/data'

export interface AccountDirectory {
  accounts: AccountRow[]
  byId: ReadonlyMap<string, AccountRow>
  resolveAccountName: (sourceId: string, fallback?: string | null) => string
}

function buildAccountMap(accounts: AccountRow[]) {
  return new Map(accounts.map((account) => [account.id, account]))
}

export function createAccountDirectory(accounts: AccountRow[]): AccountDirectory {
  const byId = buildAccountMap(accounts)
  return {
    accounts,
    byId,
    resolveAccountName: (sourceId, fallback) =>
      byId.get(sourceId)?.name ?? fallback ?? sourceId,
  }
}

export function useAccountDirectory(): AccountDirectory {
  const { data } = useAccounts()
  const accounts = data?.rows
  return useMemo(() => createAccountDirectory(accounts ?? []), [accounts])
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
