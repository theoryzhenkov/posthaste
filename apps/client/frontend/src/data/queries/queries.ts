// Typed query hooks, one per read family, over POST /api/query. Each hook is
// a plain react-query useQuery under a flat family key; the answer's
// generation stays inside the facade — components see only data. Liveness is
// not the hook's job: stream.ts invalidates every active query when the
// backend's generation advances.

import {
  useQuery,
  type QueryClient,
  type UseQueryResult,
} from '@tanstack/react-query'
import type { MailClient } from '@/data/transport/client'
import type { AccountId, AccountSettingsResult, AccountsResult, AppSettings, AppSettingsResult, MailboxCountsResult, MessageDetailQuery, MessageDetailResult, PendingOperationsResult, Query, RevLogQuery, RevLogResult, SenderAddressesResult, SmartMailboxesResult, TagsResult, ThreadQuery, ThreadView } from '@/gen'
import { useMailClient } from '../context'
import { familyKey } from '@/data/queries/queryKeys'

/** The queryFn for any family: posts the query, unwraps the envelope. */
export async function fetchQuery<T>(client: MailClient, query: Query): Promise<T> {
  const envelope = await client.query(query)
  return envelope.data as T
}

interface FamilyOptions {
  enabled?: boolean
}

function useFamilyQuery<T>(query: Query, opts?: FamilyOptions): UseQueryResult<T> {
  const client = useMailClient()
  return useQuery({
    queryKey: familyKey(query),
    queryFn: () => fetchQuery<T>(client, query),
    enabled: opts?.enabled,
  })
}

export function useThread(q: ThreadQuery, opts?: FamilyOptions): UseQueryResult<ThreadView> {
  return useFamilyQuery<ThreadView>({ thread: q }, opts)
}

export function useMessageDetail(
  q: MessageDetailQuery,
  opts?: FamilyOptions,
): UseQueryResult<MessageDetailResult> {
  return useFamilyQuery<MessageDetailResult>({ messageDetail: q }, opts)
}

export function useMailboxCounts(
  accountId?: AccountId,
  opts?: FamilyOptions,
): UseQueryResult<MailboxCountsResult> {
  return useFamilyQuery<MailboxCountsResult>({ mailboxCounts: { accountId } }, opts)
}

export function useAccounts(opts?: FamilyOptions): UseQueryResult<AccountsResult> {
  return useFamilyQuery<AccountsResult>({ accounts: {} }, opts)
}

export function useAccountSettings(
  accountId: AccountId,
  opts?: FamilyOptions,
): UseQueryResult<AccountSettingsResult> {
  return useFamilyQuery<AccountSettingsResult>({ accountSettings: { accountId } }, opts)
}

export function usePendingOperations(
  accountId?: AccountId,
  opts?: FamilyOptions,
): UseQueryResult<PendingOperationsResult> {
  return useFamilyQuery<PendingOperationsResult>({ pendingOperations: { accountId } }, opts)
}

export function useAppSettings(opts?: FamilyOptions): UseQueryResult<AppSettingsResult> {
  return useFamilyQuery<AppSettingsResult>({ appSettings: {} }, opts)
}

/**
 * The current settings document, for read-modify-write flows feeding the
 * `updateSettings` command: served from the mirror when the answer is held,
 * fetched otherwise.
 */
export async function ensureAppSettings(
  client: MailClient,
  queryClient: QueryClient,
): Promise<AppSettings> {
  const query: Query = { appSettings: {} }
  const result = await queryClient.ensureQueryData({
    queryKey: familyKey(query),
    queryFn: () => fetchQuery<AppSettingsResult>(client, query),
  })
  return result.settings
}

export function useSmartMailboxes(
  opts?: FamilyOptions,
): UseQueryResult<SmartMailboxesResult> {
  return useFamilyQuery<SmartMailboxesResult>({ smartMailboxes: {} }, opts)
}

export function useTags(
  accountId?: AccountId,
  opts?: FamilyOptions,
): UseQueryResult<TagsResult> {
  return useFamilyQuery<TagsResult>({ tags: { accountId } }, opts)
}

export function useRevLog(q: RevLogQuery, opts?: FamilyOptions): UseQueryResult<RevLogResult> {
  return useFamilyQuery<RevLogResult>({ revLog: q }, opts)
}

export function useSenderAddresses(
  accountId?: AccountId,
  opts?: FamilyOptions,
): UseQueryResult<SenderAddressesResult> {
  return useFamilyQuery<SenderAddressesResult>({ senderAddresses: { accountId } }, opts)
}
