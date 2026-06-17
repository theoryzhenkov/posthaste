import { useMemo } from 'react'
import {
  useQueries,
  useQuery,
  useQueryClient,
  type UseQueryResult,
} from '@tanstack/react-query'

import { createAccountDirectory } from './accountDirectory'
import type {
  AccountAppearance,
  AccountOverview,
  Mailbox,
  ReadResponse,
  SmartMailboxSummary,
  TagSummary,
} from './api/types'
import { queryKeys } from './queryKeys'
import { runtimeViews } from './runtime/views'

export interface MailboxNavigationSource {
  id: string
  name: string
  appearance: AccountAppearance
  mailboxes: Mailbox[]
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
 * Domain-backed data needed by mailbox navigation/search surfaces.
 *
 * The typed read-call bootstrap seeds the caches in one request; feature code
 * reads accounts, source mailboxes, smart mailboxes, and tags from normalized
 * query keys.
 *
 * @spec docs/eph/DESIGN-L1-client-read-models#domain-authority
 */
export function useMailboxNavigationReadModels(): MailboxNavigationReadModels {
  const bootstrapQuery = useMailNavigationReadBootstrap()
  const accountsQuery = useQuery<AccountOverview[]>({
    queryKey: queryKeys.accounts,
    queryFn: () => Promise.resolve([]),
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
