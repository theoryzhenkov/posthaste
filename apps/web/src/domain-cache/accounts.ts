import type { QueryClient } from '@tanstack/react-query'

import type {
  AccountOverview,
  AccountRuntime,
  DomainEvent,
  SyncProgress,
} from '../api/types'
import { queryKeys } from '../queryKeys'
import { invalidateAccountReadModels } from './invalidations'

function isAccountStatus(value: unknown): value is AccountRuntime['status'] {
  return (
    value === 'ready' ||
    value === 'syncing' ||
    value === 'degraded' ||
    value === 'authError' ||
    value === 'offline' ||
    value === 'disabled'
  )
}

function isPushStatus(value: unknown): value is AccountRuntime['push'] {
  return (
    value === 'connected' ||
    value === 'reconnecting' ||
    value === 'unsupported' ||
    value === 'disabled'
  )
}

function isStringOrNull(value: unknown): value is string | null {
  return value === null || typeof value === 'string'
}

function isNumberOrNull(value: unknown): value is number | null {
  return value === null || typeof value === 'number'
}

function isSyncProgress(value: unknown): value is SyncProgress {
  if (typeof value !== 'object' || value === null) {
    return false
  }

  const progress = value as Record<string, unknown>
  return (
    typeof progress.syncId === 'string' &&
    (progress.trigger === 'startup' ||
      progress.trigger === 'poll' ||
      progress.trigger === 'push' ||
      progress.trigger === 'manual') &&
    typeof progress.startedAt === 'string' &&
    (progress.stage === 'connecting' ||
      progress.stage === 'discovering' ||
      progress.stage === 'planning' ||
      progress.stage === 'fetching' ||
      progress.stage === 'storing' ||
      progress.stage === 'waiting') &&
    typeof progress.detail === 'string' &&
    isStringOrNull(progress.mailboxName) &&
    isNumberOrNull(progress.mailboxIndex) &&
    isNumberOrNull(progress.mailboxCount) &&
    isNumberOrNull(progress.messageCount) &&
    isNumberOrNull(progress.totalCount)
  )
}

/**
 * Build a partial runtime patch from an `account.status_changed` payload.
 *
 * Tolerant by design: every valid field present is applied; missing or
 * malformed fields are simply skipped. This avoids the previous all-or-nothing
 * behaviour where a single shape drift discarded the whole live update and fell
 * back to a query invalidation that often did not refetch.
 */
function runtimePatchFromPayload(
  payload: DomainEvent['payload'],
): Partial<AccountRuntime> {
  const patch: Partial<AccountRuntime> = {}
  if (isAccountStatus(payload.status)) {
    patch.status = payload.status
  }
  if (isPushStatus(payload.push)) {
    patch.push = payload.push
  }
  if (isStringOrNull(payload.lastSyncAt)) {
    patch.lastSyncAt = payload.lastSyncAt
  }
  if (isStringOrNull(payload.lastSyncError)) {
    patch.lastSyncError = payload.lastSyncError
  }
  if (isStringOrNull(payload.lastSyncErrorCode)) {
    patch.lastSyncErrorCode = payload.lastSyncErrorCode
  }
  if (payload.syncProgress === null || isSyncProgress(payload.syncProgress)) {
    patch.syncProgress = payload.syncProgress
  }
  return patch
}

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

export function applyAccountStatusPatch(
  queryClient: QueryClient,
  accountId: string,
  payload: DomainEvent['payload'],
): boolean {
  const patch = runtimePatchFromPayload(payload)
  if (Object.keys(patch).length === 0) {
    return false
  }

  const applyRuntime = (account: AccountOverview): AccountOverview => ({
    ...account,
    runtime: { ...account.runtime, ...patch },
  })

  queryClient.setQueryData<AccountOverview[]>(
    queryKeys.accounts,
    (current = []) =>
      current.map((account) =>
        account.id === accountId ? applyRuntime(account) : account,
      ),
  )
  queryClient.setQueryData<AccountOverview>(
    queryKeys.account(accountId),
    (current) => (current ? applyRuntime(current) : current),
  )
  return true
}
