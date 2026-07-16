import { useEffect, useMemo } from 'react'
import {
  useQueries,
  useQuery,
  useQueryClient,
  type QueryClient,
  type UseQueryResult,
} from '@tanstack/react-query'

import { runtimeLinkClient } from './runtime/linkClient'
import { createAccountDirectory } from './accountDirectory'
import type {
  AccountAppearance,
  AccountOverview,
  Mailbox,
  ReadResponse,
  SmartMailboxSummary,
  TagSummary,
} from './api/types'
import { accountHealthFor, type AccountHealth } from './accountHealth'
import { queryKeys } from './queryKeys'
import { runtimeViews } from './runtime/views'

export interface MailboxNavigationSource {
  id: string
  name: string
  appearance: AccountAppearance
  mailboxes: Mailbox[]
  /** Classified account health, so the sidebar row can surface degraded/error
   * state + a recovery affordance without re-deriving it (M45). */
  health: AccountHealth
}

export interface MailboxNavigationReadModels {
  error: Error | null
  isLoading: boolean
  refetchBootstrap: UseQueryResult<ReadResponse, Error>['refetch']
  smartMailboxes: SmartMailboxSummary[]
  sources: MailboxNavigationSource[]
  tags: TagSummary[]
}

function isUserTag(tag: TagSummary): boolean {
  const name = tag.name.trim()
  return Boolean(name) && !name.startsWith('$')
}

/**
 * Client-owned typed read-call operation for mail navigation bootstrap.
 *
 * The backend exposes domain read operations; the client composes them into the
 * graph it needs and hydrates domain-named React Query caches from the result.
 *
 * @spec docs/L1-api#read-calls
 * @spec docs/eph/DESIGN-L1-client-read-models#aggregate-hydration
 */
export function useMailNavigationReadBootstrap() {
  const queryClient = useQueryClient()
  return useQuery({
    queryKey: queryKeys.mailNavigationRead,
    queryFn: async () => {
      const response = await runtimeViews.mail.read({
        calls: [
          { id: 'accounts', op: 'Account/list' },
          {
            id: 'mailboxes',
            op: 'Mailbox/list',
            args: { accountIds: '#accounts.enabledIds' },
          },
          { id: 'smartMailboxes', op: 'SmartMailbox/list' },
          {
            id: 'tags',
            op: 'Tag/list',
            args: { accountIds: '#accounts.enabledIds' },
          },
        ],
      })
      hydrateMailNavigationRead(queryClient, response)
      return response
    },
    staleTime: 30_000,
  })
}

function hydrateMailNavigationRead(
  queryClient: ReturnType<typeof useQueryClient>,
  response: ReadResponse,
) {
  const accounts = response.results.accounts
  if (accounts?.op === 'Account/list') {
    queryClient.setQueryData<AccountOverview[]>(
      queryKeys.accounts,
      accounts.value.items,
    )
    for (const account of accounts.value.items) {
      queryClient.setQueryData(queryKeys.account(account.id), account)
    }
  }

  const mailboxes = response.results.mailboxes
  if (mailboxes?.op === 'Mailbox/list') {
    for (const [accountId, accountMailboxes] of Object.entries(
      mailboxes.value.byAccountId,
    )) {
      queryClient.setQueryData(queryKeys.mailboxes(accountId), accountMailboxes)
    }
  }

  const smartMailboxes = response.results.smartMailboxes
  if (smartMailboxes?.op === 'SmartMailbox/list') {
    queryClient.setQueryData(
      queryKeys.smartMailboxes,
      smartMailboxes.value.items,
    )
  }

  const tags = response.results.tags
  if (tags?.op === 'Tag/list') {
    queryClient.setQueryData(queryKeys.tags, tags.value.items)
  }
}

/**
 * C1 / D113 (RC2): reconcile source + smart mailbox COUNTS on the M44
 * recovery edge — the completion of M44's reconcile pass for counts.
 *
 * The M44 recovery-edge reconcile (`runtimeLinkClient.onLinkReestablished`)
 * re-serves view ROWS but never counts; an invalidation missed during the
 * disconnect would otherwise leave a count stale until the next
 * count-affecting event. On the recovery edge we refetch the authoritative
 * counts — the source `mailboxes(accountId)` queries AND the `smartMailboxes`
 * query, the same react-query keys every count consumer reads
 * (RFC-L2-count-unification) — so a count that drifted during the gap heals
 * without a reload. Post-countDelta there is no separate live-count owner to
 * reseed: the refetched query data IS the count.
 *
 * This fires ONLY on `onLinkReestablished`, never on a normal mutation, so the
 * steady-state event-driven invalidations (the fast path) are untouched.
 */
export async function reconcileMailboxCountsOnRecovery(
  queryClient: QueryClient,
  accountIds: readonly string[],
): Promise<void> {
  // Refetch the authoritative counts against the FRESH link. `type: 'active'`
  // drives the mounted sidebar observers' queryFns. A refetch failure on the
  // recovery edge must not surface as an unhandled rejection — the next
  // count-affecting event re-invalidates anyway.
  await Promise.all([
    ...accountIds.map((accountId) =>
      queryClient.refetchQueries({
        queryKey: queryKeys.mailboxes(accountId),
        type: 'active',
      }),
    ),
    queryClient.refetchQueries({
      queryKey: queryKeys.smartMailboxes,
      type: 'active',
    }),
  ]).catch(() => {})
}

/**
 * Domain-backed data needed by mailbox navigation/search surfaces.
 *
 * The typed read-call bootstrap seeds the caches in one request; feature code
 * reads accounts, source mailboxes, smart mailboxes, and tags from normalized
 * query keys.
 *
 * @spec docs/eph/DESIGN-L1-client-read-models#domain-authority
 */
export function useMailboxNavigationReadModels(): MailboxNavigationReadModels {
  const queryClient = useQueryClient()
  const bootstrapQuery = useMailNavigationReadBootstrap()
  // Read-only observer of the shared accounts cache (seeded by the bootstrap
  // hydrate / accountStatus view). It must use the SAME queryFn as every other
  // `queryKeys.accounts` observer: a divergent `() => Promise.resolve([])` here
  // could win a refetch triggered elsewhere (e.g. a background sync
  // invalidation) and resolve the shared query to `[]`, briefly emptying
  // accounts everywhere.
  const accountsQuery = useQuery<AccountOverview[]>({
    queryKey: queryKeys.accounts,
    queryFn: runtimeViews.accounts.list,
    enabled: false,
  })
  const accountDirectory = useMemo(
    () => createAccountDirectory(accountsQuery.data ?? []),
    [accountsQuery.data],
  )
  const enabledAccounts = useMemo(
    () => accountDirectory.accounts.filter((account) => account.enabled),
    [accountDirectory.accounts],
  )
  const enabledAccountIds = useMemo(
    () => enabledAccounts.map((account) => account.id),
    [enabledAccounts],
  )

  // C1/D113 (RC2): reconcile counts on the SAME M44 recovery edge the view
  // re-open uses (`onLinkReestablished`) — no separate trigger. On a fresh link
  // (reap/sleep/reconnect) refetch the source + smart counts and reseed the
  // live-store owner so a count that drifted during the gap heals without a
  // reload. Re-registers only when the enabled-account set changes.
  useEffect(
    () =>
      runtimeLinkClient.onLinkReestablished(() => {
        void reconcileMailboxCountsOnRecovery(queryClient, enabledAccountIds)
      }),
    [queryClient, enabledAccountIds],
  )

  const mailboxQueries = useQueries({
    queries: enabledAccounts.map((account) => ({
      queryKey: queryKeys.mailboxes(account.id),
      queryFn: () => runtimeViews.mail.mailboxes(account.id),
      enabled: bootstrapQuery.isSuccess,
      staleTime: 30_000,
    })),
  })

  const smartMailboxesQuery = useQuery({
    queryKey: queryKeys.smartMailboxes,
    queryFn: runtimeViews.smartMailboxes.list,
    enabled: bootstrapQuery.isSuccess,
    staleTime: 30_000,
  })

  const tagsQuery = useQuery<TagSummary[]>({
    queryKey: queryKeys.tags,
    queryFn: () => Promise.resolve([]),
    enabled: false,
  })

  const sources = useMemo(
    () =>
      enabledAccounts.map((account, index) => ({
        id: account.id,
        name: accountDirectory.resolveAccountName(account.id, account.name),
        appearance: account.appearance,
        mailboxes: mailboxQueries[index]?.data ?? [],
        health: accountHealthFor(account),
      })),
    [accountDirectory, enabledAccounts, mailboxQueries],
  )

  const mailboxError = mailboxQueries.find(
    (query): query is UseQueryResult<Mailbox[], Error> => query.error !== null,
  )?.error
  const error =
    bootstrapQuery.error ?? smartMailboxesQuery.error ?? mailboxError ?? null
  const isLoading =
    bootstrapQuery.isLoading ||
    smartMailboxesQuery.isLoading ||
    mailboxQueries.some((query) => query.isLoading)

  return {
    error,
    isLoading,
    refetchBootstrap: bootstrapQuery.refetch,
    smartMailboxes: smartMailboxesQuery.data ?? [],
    sources,
    tags: (tagsQuery.data ?? []).filter(isUserTag),
  }
}
