/**
 * Centralized React Query cache updates for domain events and mutations.
 *
 * @spec docs/L1-ui#data-fetching
 * @spec docs/L1-api#sse-event-stream
 */
import type { QueryClient } from '@tanstack/react-query'
import type { AccountOverview, DomainEvent, SyncProgress } from './api/types'
import {
  applyKeywordEventPatch,
  findConversationIdForMessage,
  mailKeys,
} from './mailState'
import { EVENT_TOPICS, type DomainEventTopic } from './domainVocabulary'
import { queryKeys } from './queryKeys'

function isStringArray(value: unknown): value is string[] {
  return Array.isArray(value) && value.every((item) => typeof item === 'string')
}

function payloadConversationId(payload: DomainEvent['payload']): string | null {
  return typeof payload.conversationId === 'string'
    ? payload.conversationId
    : null
}

function eventTarget(event: DomainEvent) {
  return event.messageId && event.accountId
    ? { messageId: event.messageId, sourceId: event.accountId }
    : null
}

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

function invalidateMessageListReadModels(queryClient: QueryClient) {
  void queryClient.invalidateQueries({ queryKey: queryKeys.messagesRoot })
  void queryClient.invalidateQueries({ queryKey: queryKeys.conversationsRoot })
}

function invalidateMessageDetailReadModels(queryClient: QueryClient) {
  void queryClient.invalidateQueries({ queryKey: mailKeys.messageRoot })
  void queryClient.invalidateQueries({ queryKey: mailKeys.conversationRoot })
  void queryClient.invalidateQueries({
    queryKey: mailKeys.conversationSummaryRoot,
  })
}

function invalidateSmartMailboxReadModels(
  queryClient: QueryClient,
  smartMailboxId?: string | null,
) {
  void queryClient.invalidateQueries({ queryKey: queryKeys.smartMailboxes })
  if (smartMailboxId) {
    void queryClient.invalidateQueries({
      queryKey: queryKeys.smartMailbox(smartMailboxId),
    })
  } else {
    void queryClient.invalidateQueries({ queryKey: ['smart-mailbox'] })
  }
  void queryClient.invalidateQueries({ queryKey: queryKeys.sidebar })
  invalidateMessageListReadModels(queryClient)
}

function invalidateMailboxReadModels(
  queryClient: QueryClient,
  accountId: string,
) {
  void queryClient.invalidateQueries({ queryKey: queryKeys.sidebar })
  void queryClient.invalidateQueries({
    queryKey: queryKeys.mailboxes(accountId),
  })
  void queryClient.invalidateQueries({ queryKey: queryKeys.smartMailboxes })
  invalidateMessageListReadModels(queryClient)
}

function invalidateAccountRuntimeReadModels(
  queryClient: QueryClient,
  accountId: string,
) {
  void queryClient.invalidateQueries({ queryKey: queryKeys.accounts })
  void queryClient.invalidateQueries({ queryKey: queryKeys.account(accountId) })
}

export function invalidateAccountReadModels(
  queryClient: QueryClient,
  accountId?: string,
) {
  void queryClient.invalidateQueries({ queryKey: queryKeys.settings })
  void queryClient.invalidateQueries({ queryKey: queryKeys.accounts })
  if (accountId) {
    void queryClient.invalidateQueries({
      queryKey: queryKeys.account(accountId),
    })
    void queryClient.invalidateQueries({
      queryKey: queryKeys.mailboxes(accountId),
    })
  }
  void queryClient.invalidateQueries({ queryKey: queryKeys.sidebar })
  invalidateMessageListReadModels(queryClient)
}

export function applyAccountMutationResult(
  queryClient: QueryClient,
  account: AccountOverview,
) {
  mergeAccountOverview(queryClient, account)
  invalidateAccountReadModels(queryClient, account.id)
}

function applyAccountStatusPatch(
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

function invalidateTargetMessageReadModels(
  queryClient: QueryClient,
  event: DomainEvent,
) {
  const target = eventTarget(event)
  if (!target) {
    invalidateMessageDetailReadModels(queryClient)
    return
  }
  void queryClient.invalidateQueries({
    queryKey: mailKeys.message(target.sourceId, target.messageId),
  })
  const conversationId =
    payloadConversationId(event.payload) ??
    findConversationIdForMessage(queryClient, target)
  if (conversationId) {
    void queryClient.invalidateQueries({
      queryKey: mailKeys.conversation(conversationId),
    })
    void queryClient.invalidateQueries({
      queryKey: mailKeys.conversationSummary(conversationId),
    })
  }
}

type EventHandler = (queryClient: QueryClient, event: DomainEvent) => void

const eventHandlers = {
  [EVENT_TOPICS.SettingsUpdated]: (queryClient) => {
    invalidateAccountReadModels(queryClient)
    invalidateSmartMailboxReadModels(queryClient)
  },
  [EVENT_TOPICS.ConfigReloaded]: (queryClient) => {
    invalidateAccountReadModels(queryClient)
    invalidateSmartMailboxReadModels(queryClient)
    invalidateMessageDetailReadModels(queryClient)
  },
  [EVENT_TOPICS.SmartMailboxCreated]: (queryClient, event) => {
    invalidateSmartMailboxReadModels(
      queryClient,
      event.payload.smartMailboxId as string | undefined,
    )
  },
  [EVENT_TOPICS.SmartMailboxUpdated]: (queryClient, event) => {
    invalidateSmartMailboxReadModels(
      queryClient,
      event.payload.smartMailboxId as string | undefined,
    )
  },
  [EVENT_TOPICS.SmartMailboxDeleted]: (queryClient, event) => {
    const smartMailboxId = event.payload.smartMailboxId as string | undefined
    if (smartMailboxId) {
      queryClient.removeQueries({
        queryKey: queryKeys.smartMailbox(smartMailboxId),
        exact: true,
      })
    }
    invalidateSmartMailboxReadModels(queryClient, smartMailboxId)
  },
  [EVENT_TOPICS.SmartMailboxReset]: (queryClient) => {
    invalidateSmartMailboxReadModels(queryClient)
  },
  [EVENT_TOPICS.SyncCompleted]: (queryClient, event) => {
    invalidateAccountReadModels(queryClient, event.accountId)
    invalidateSmartMailboxReadModels(queryClient)
    invalidateMessageDetailReadModels(queryClient)
  },
  [EVENT_TOPICS.SyncFailed]: (queryClient, event) => {
    invalidateAccountRuntimeReadModels(queryClient, event.accountId)
  },
  [EVENT_TOPICS.AccountCreated]: (queryClient, event) => {
    invalidateAccountReadModels(queryClient, event.accountId)
  },
  [EVENT_TOPICS.AccountStatusChanged]: (queryClient, event) => {
    if (!applyAccountStatusPatch(queryClient, event.accountId, event.payload)) {
      invalidateAccountRuntimeReadModels(queryClient, event.accountId)
    }
  },
  [EVENT_TOPICS.AccountUpdated]: (queryClient, event) => {
    invalidateAccountReadModels(queryClient, event.accountId)
  },
  [EVENT_TOPICS.AccountDeleted]: (queryClient, event) => {
    removeAccountOverview(queryClient, event.accountId)
    invalidateAccountReadModels(queryClient)
    invalidateSmartMailboxReadModels(queryClient)
    invalidateMessageDetailReadModels(queryClient)
  },
  [EVENT_TOPICS.MailboxUpdated]: (queryClient, event) => {
    invalidateMailboxReadModels(queryClient, event.accountId)
  },
  [EVENT_TOPICS.MessageArrived]: (queryClient) => {
    void queryClient.invalidateQueries({ queryKey: queryKeys.sidebar })
    void queryClient.invalidateQueries({ queryKey: queryKeys.smartMailboxes })
    invalidateMessageListReadModels(queryClient)
  },
  [EVENT_TOPICS.MessageKeywordsChanged]: (queryClient, event) => {
    void queryClient.invalidateQueries({ queryKey: queryKeys.sidebar })
    void queryClient.invalidateQueries({ queryKey: queryKeys.smartMailboxes })
    invalidateMessageListReadModels(queryClient)

    const target = eventTarget(event)
    const keywords = event.payload.keywords
    const patched =
      target && isStringArray(keywords)
        ? applyKeywordEventPatch(queryClient, target, keywords)
        : false
    if (!patched) {
      invalidateTargetMessageReadModels(queryClient, event)
    }
  },
  [EVENT_TOPICS.MessageBodyCached]: (queryClient, event) => {
    invalidateTargetMessageReadModels(queryClient, event)
  },
  [EVENT_TOPICS.MessageMailboxesChanged]: (queryClient, event) => {
    void queryClient.invalidateQueries({ queryKey: queryKeys.sidebar })
    void queryClient.invalidateQueries({ queryKey: queryKeys.smartMailboxes })
    invalidateMessageListReadModels(queryClient)
    invalidateTargetMessageReadModels(queryClient, event)
  },
  [EVENT_TOPICS.MessageUpdated]: (queryClient, event) => {
    invalidateMessageListReadModels(queryClient)
    invalidateTargetMessageReadModels(queryClient, event)
  },
  [EVENT_TOPICS.PushConnected]: (queryClient, event) => {
    invalidateAccountRuntimeReadModels(queryClient, event.accountId)
  },
  [EVENT_TOPICS.PushDisconnected]: (queryClient, event) => {
    invalidateAccountRuntimeReadModels(queryClient, event.accountId)
  },
} satisfies Record<DomainEventTopic, EventHandler>

function isDomainEventTopic(topic: string): topic is DomainEventTopic {
  return Object.values(EVENT_TOPICS).includes(topic as DomainEventTopic)
}

export function applyDomainEvent(queryClient: QueryClient, event: DomainEvent) {
  if (!isDomainEventTopic(event.topic)) {
    invalidateAccountReadModels(queryClient)
    return
  }
  eventHandlers[event.topic](queryClient, event)
}
