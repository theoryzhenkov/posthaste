import type { QueryClient } from '@tanstack/react-query'

import type { DomainEvent } from '../api/types'
import { queryKeys } from '../queryKeys'
import { removeAccountOverview } from './accounts'
import {
  invalidateAccountReadModels,
  invalidateMailboxReadModels,
  invalidateMailNavigationBootstrapReadModels,
  invalidateMessageDetailReadModels,
  invalidateMessageListReadModels,
  invalidateSmartMailboxReadModels,
} from './invalidations'

export type EventHandler = (queryClient: QueryClient, event: DomainEvent) => void

interface ResourceChange {
  accountId?: string
  id?: string
  kind: string
  operation: string
}

function isResourceChange(value: unknown): value is ResourceChange {
  if (typeof value !== 'object' || value === null) {
    return false
  }
  const resource = value as Record<string, unknown>
  return (
    typeof resource.kind === 'string' &&
    typeof resource.operation === 'string' &&
    (resource.id === undefined || typeof resource.id === 'string') &&
    (resource.accountId === undefined || typeof resource.accountId === 'string')
  )
}

function resourceChangesFromPayload(
  payload: DomainEvent['payload'],
): ResourceChange[] {
  return Array.isArray(payload.resources)
    ? payload.resources.filter(isResourceChange)
    : []
}

function applyResourceInvalidation(
  queryClient: QueryClient,
  event: DomainEvent,
  resource: ResourceChange,
): boolean {
  const accountId = resource.accountId ?? event.accountId
  switch (resource.kind) {
    case 'appSettings':
      invalidateAccountReadModels(queryClient)
      invalidateSmartMailboxReadModels(queryClient)
      return true
    case 'config':
      invalidateAccountReadModels(queryClient)
      invalidateSmartMailboxReadModels(queryClient)
      invalidateMailNavigationBootstrapReadModels(queryClient)
      invalidateMessageDetailReadModels(queryClient)
      return true
    case 'account': {
      const targetAccountId = resource.id ?? accountId
      if (resource.operation === 'deleted') {
        removeAccountOverview(queryClient, targetAccountId)
        invalidateAccountReadModels(queryClient)
        invalidateSmartMailboxReadModels(queryClient)
        invalidateMessageDetailReadModels(queryClient)
        return true
      }
      invalidateAccountReadModels(queryClient, targetAccountId)
      return true
    }
    case 'smartMailbox':
      if (resource.operation === 'deleted' && resource.id) {
        queryClient.removeQueries({
          queryKey: queryKeys.smartMailbox(resource.id),
          exact: true,
        })
      }
      invalidateSmartMailboxReadModels(queryClient, resource.id)
      return true
    case 'mailbox':
      invalidateMailboxReadModels(queryClient, accountId)
      return true
    case 'sync':
      invalidateAccountReadModels(queryClient, accountId)
      invalidateSmartMailboxReadModels(queryClient)
      invalidateMailNavigationBootstrapReadModels(queryClient)
      invalidateMessageDetailReadModels(queryClient)
      return true
    case 'message':
      invalidateMessageListReadModels(queryClient)
      return true
    default:
      return false
  }
}

function applyResourceInvalidations(
  queryClient: QueryClient,
  event: DomainEvent,
): boolean {
  return resourceChangesFromPayload(event.payload)
    .map((resource) => applyResourceInvalidation(queryClient, event, resource))
    .some(Boolean)
}

export function applyResourceInvalidationsOrFallback(
  queryClient: QueryClient,
  event: DomainEvent,
  fallback: EventHandler,
) {
  if (!applyResourceInvalidations(queryClient, event)) {
    fallback(queryClient, event)
  }
}
