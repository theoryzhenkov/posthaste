import type { QueryClient } from '@tanstack/react-query'

import type { AccountOverview, DomainEvent, SyncProgress } from '../api/types'
import { queryKeys } from '../queryKeys'
import { invalidateAccountReadModels } from './invalidations'

function isAccountStatus(value: unknown): value is AccountOverview['status'] {
  return (
    value === 'ready' ||
    value === 'syncing' ||
    value === 'degraded' ||
    value === 'authError' ||
    value === 'offline' ||
    value === 'disabled'
  )
}

function isPushStatus(value: unknown): value is AccountOverview['push'] {
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

function statusPatchFromPayload(payload: DomainEvent['payload']) {
  if (!isAccountStatus(payload.status) || !isPushStatus(payload.push)) {
    return null
  }

  if (payload.syncProgress !== null && !isSyncProgress(payload.syncProgress)) {
    return null
  }

  if (
    !isStringOrNull(payload.lastSyncAt) ||
    !isStringOrNull(payload.lastSyncError) ||
    !isStringOrNull(payload.lastSyncErrorCode)
  ) {
    return null
  }

  return {
    status: payload.status,
    push: payload.push,
    lastSyncAt: payload.lastSyncAt,
    lastSyncError: payload.lastSyncError,
    lastSyncErrorCode: payload.lastSyncErrorCode,
    syncProgress: payload.syncProgress,
  }
}

function mergeAccountRuntime(
  current: AccountOverview | undefined,
  next: AccountOverview,
): AccountOverview {
  if (!current || next.status !== 'syncing' || current.status === 'syncing') {
    return next
  }

  return {
    ...next,
    status: current.status,
    push: current.push,
    lastSyncAt: current.lastSyncAt,
    lastSyncError: current.lastSyncError,
    lastSyncErrorCode: current.lastSyncErrorCode,
    syncProgress: current.syncProgress,
  }
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
          ? mergeAccountRuntime(candidate, account)
          : candidate,
      )
    },
  )
  queryClient.setQueryData<AccountOverview>(
    queryKeys.account(account.id),
    (current) => mergeAccountRuntime(current, account),
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
  const patch = statusPatchFromPayload(payload)
  if (!patch) {
    return false
  }

  queryClient.setQueryData<AccountOverview[]>(
    queryKeys.accounts,
    (current = []) =>
      current.map((account) =>
        account.id === accountId ? { ...account, ...patch } : account,
      ),
  )
  queryClient.setQueryData<AccountOverview>(
    queryKeys.account(accountId),
    (current) => (current ? { ...current, ...patch } : current),
  )
  return true
}
