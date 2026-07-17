/**
 * Domain-backed data needed by mailbox navigation/search surfaces, composed
 * from four query families: `accounts` (identity + health), `mailboxCounts`
 * (per-account mailbox rows with live counters), `smartMailboxes`, and
 * `tags`. Per-account appearance rides on the `accountSettings` family, with
 * a name-derived fallback until it arrives.
 *
 * Liveness is not composed here: every underlying query is invalidated
 * globally on generation advance (see data/stream.ts), so the sidebar heals
 * on events, reconnect, and backend restart without any recovery hook.
 */
import { useCallback, useMemo } from 'react'
import { useQueries } from '@tanstack/react-query'

import { createAccountDirectory } from './accountDirectory'
import type {
  AccountAppearance,
  AccountRow,
  AccountSettingsResult,
  MailboxSummary,
  SmartMailboxRow,
  TagSummary,
} from '@/gen'
import { accountHealthFor, type AccountHealth } from './accountHealth'
import {
  fetchQuery,
  useAccounts,
  useMailboxCounts,
  useMailClient,
  useSmartMailboxes,
  useTags,
} from '@/data'
import { familyKey } from '@/data/queryKeys'

export interface MailboxNavigationSource {
  id: string
  name: string
  appearance: AccountAppearance
  mailboxes: MailboxSummary[]
  /** Classified account health, so the sidebar row can surface degraded/error
   * state + a recovery affordance without re-deriving it. */
  health: AccountHealth
}

export interface MailboxNavigationReadModels {
  error: Error | null
  isLoading: boolean
  /** Retries the composed navigation queries after a failure. */
  refetchBootstrap: () => Promise<unknown>
  smartMailboxes: SmartMailboxRow[]
  sources: MailboxNavigationSource[]
  tags: TagSummary[]
}

function isUserTag(tag: TagSummary): boolean {
  const name = tag.name.trim()
  return Boolean(name) && !name.startsWith('$')
}

/** Stable name-derived appearance used until the account's configured
 * appearance (an `accountSettings` answer) arrives, and for accounts without
 * one. Matches the server's derivation: word initials + an id-hashed hue. */
export function fallbackAppearance(account: {
  id: string
  name: string
}): AccountAppearance {
  const words = account.name
    .split(/\s+/)
    .map((word) => word.trim())
    .filter(Boolean)
  const initials =
    words
      .slice(0, 2)
      .map((word) => word.charAt(0))
      .join('')
      .toUpperCase() || '?'
  let hash = 0
  for (let i = 0; i < account.id.length; i++) {
    hash = (hash * 31 + account.id.charCodeAt(i)) >>> 0
  }
  return { kind: 'initials', initials, colorHue: hash % 360 }
}

/**
 * The navigation read models: enabled accounts with their mailboxes and
 * health, smart mailboxes, and user tags — every field a query answer.
 */
export function useMailboxNavigationReadModels(): MailboxNavigationReadModels {
  const client = useMailClient()
  const accountsQuery = useAccounts()
  const countsQuery = useMailboxCounts()
  const smartMailboxesQuery = useSmartMailboxes()
  const tagsQuery = useTags()

  const accountRows = accountsQuery.data?.rows
  const accountDirectory = useMemo(
    () => createAccountDirectory(accountRows ?? []),
    [accountRows],
  )
  const enabledAccounts = useMemo(
    () => accountDirectory.accounts.filter((account) => account.enabled),
    [accountDirectory.accounts],
  )

  // Appearance lives on the accountSettings family (one query per enabled
  // account). Non-blocking: sources render with the fallback until it lands.
  const appearanceQueries = useQueries({
    queries: enabledAccounts.map((account) => ({
      queryKey: familyKey({ accountSettings: { accountId: account.id } }),
      queryFn: () =>
        fetchQuery<AccountSettingsResult>(client, {
          accountSettings: { accountId: account.id },
        }),
    })),
  })

  const countRows = countsQuery.data?.rows
  const mailboxesByAccount = useMemo(() => {
    const byAccount = new Map<string, MailboxSummary[]>()
    for (const row of countRows ?? []) {
      const list = byAccount.get(row.accountId)
      if (list) {
        list.push(row.mailbox)
      } else {
        byAccount.set(row.accountId, [row.mailbox])
      }
    }
    return byAccount
  }, [countRows])

  const sources = useMemo(
    () =>
      enabledAccounts.map(
        (account: AccountRow, index): MailboxNavigationSource => ({
          id: account.id,
          name: accountDirectory.resolveAccountName(account.id, account.name),
          appearance:
            appearanceQueries[index]?.data?.appearance ??
            fallbackAppearance(account),
          mailboxes: mailboxesByAccount.get(account.id) ?? [],
          health: accountHealthFor(account),
        }),
      ),
    [accountDirectory, enabledAccounts, appearanceQueries, mailboxesByAccount],
  )

  const smartMailboxRows = smartMailboxesQuery.data?.rows
  const tagRows = tagsQuery.data?.rows
  const tags = useMemo(
    () => (tagRows ?? []).filter(isUserTag),
    [tagRows],
  )

  const error =
    accountsQuery.error ??
    countsQuery.error ??
    smartMailboxesQuery.error ??
    tagsQuery.error ??
    null
  const isLoading =
    accountsQuery.isLoading ||
    countsQuery.isLoading ||
    smartMailboxesQuery.isLoading

  const refetchAccounts = accountsQuery.refetch
  const refetchCounts = countsQuery.refetch
  const refetchSmart = smartMailboxesQuery.refetch
  const refetchTags = tagsQuery.refetch
  const refetchBootstrap = useCallback(
    () =>
      Promise.all([
        refetchAccounts(),
        refetchCounts(),
        refetchSmart(),
        refetchTags(),
      ]),
    [refetchAccounts, refetchCounts, refetchSmart, refetchTags],
  )

  return {
    error,
    isLoading,
    refetchBootstrap,
    smartMailboxes: smartMailboxRows ?? [],
    sources,
    tags,
  }
}
