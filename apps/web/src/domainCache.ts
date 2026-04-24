/**
 * Centralized React Query cache updates for domain events and mutations.
 *
 * @spec docs/L1-ui#data-fetching
 * @spec docs/L1-api#sse-event-stream
 */
import type { QueryClient } from '@tanstack/react-query'
import type { AccountOverview, DomainEvent } from './api/types'
import {
  applyKeywordEventPatch,
  findConversationIdForMessage,
  mailKeys,
} from './mailState'
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
        candidate.id === account.id ? account : candidate,
      )
    },
  )
  queryClient.setQueryData(queryKeys.account(account.id), account)
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
  }
  void queryClient.invalidateQueries({ queryKey: queryKeys.sidebar })
  void queryClient.invalidateQueries({ queryKey: queryKeys.messagesRoot })
}

export function applyAccountMutationResult(
  queryClient: QueryClient,
  account: AccountOverview,
) {
  mergeAccountOverview(queryClient, account)
  invalidateAccountReadModels(queryClient, account.id)
}

function applyMessageEvent(queryClient: QueryClient, event: DomainEvent) {
  const target = eventTarget(event)

  switch (event.topic) {
    case 'message.arrived': {
      void queryClient.invalidateQueries({ queryKey: queryKeys.sidebar })
      void queryClient.invalidateQueries({ queryKey: queryKeys.smartMailboxes })
      return
    }
    case 'message.keywords_changed': {
      void queryClient.invalidateQueries({ queryKey: queryKeys.sidebar })
      void queryClient.invalidateQueries({ queryKey: queryKeys.smartMailboxes })

      const keywords = event.payload.keywords
      const patched =
        target && isStringArray(keywords)
          ? applyKeywordEventPatch(queryClient, target, keywords)
          : false

      if (target && !patched) {
        void queryClient.invalidateQueries({
          queryKey: mailKeys.message(target.sourceId, target.messageId),
        })
        const conversationId = findConversationIdForMessage(queryClient, target)
        if (conversationId) {
          void queryClient.invalidateQueries({
            queryKey: mailKeys.conversation(conversationId),
          })
          void queryClient.invalidateQueries({
            queryKey: mailKeys.conversationSummary(conversationId),
          })
        }
      }
      return
    }
    case 'message.mailboxes_changed': {
      void queryClient.invalidateQueries({ queryKey: queryKeys.sidebar })
      void queryClient.invalidateQueries({ queryKey: queryKeys.smartMailboxes })
      if (target) {
        void queryClient.invalidateQueries({
          queryKey: mailKeys.message(target.sourceId, target.messageId),
        })
        const conversationId = findConversationIdForMessage(queryClient, target)
        if (conversationId) {
          void queryClient.invalidateQueries({
            queryKey: mailKeys.conversation(conversationId),
          })
          void queryClient.invalidateQueries({
            queryKey: mailKeys.conversationSummary(conversationId),
          })
        }
      }
      return
    }
    case 'message.updated': {
      if (target) {
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
    }
  }
}

export function applyDomainEvent(queryClient: QueryClient, event: DomainEvent) {
  switch (event.topic) {
    case 'account.created':
    case 'account.updated':
    case 'account.status_changed': {
      invalidateAccountReadModels(queryClient, event.accountId)
      return
    }
    case 'account.deleted': {
      removeAccountOverview(queryClient, event.accountId)
      invalidateAccountReadModels(queryClient)
      return
    }
    default:
      applyMessageEvent(queryClient, event)
  }
}
