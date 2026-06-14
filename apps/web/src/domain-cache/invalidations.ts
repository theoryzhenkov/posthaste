import type { QueryClient } from '@tanstack/react-query'

import type { DomainEvent, SourceMessageRef } from '../api/types'
import {
  findConversationIdForMessage,
  mailKeys,
} from '../mailState'
import { queryKeys } from '../queryKeys'
import { eventTarget, payloadConversationId } from './payload'

export function invalidateMessageListReadModels(queryClient: QueryClient) {
  void queryClient.invalidateQueries({ queryKey: queryKeys.messagesRoot })
  void queryClient.invalidateQueries({ queryKey: queryKeys.conversationsRoot })
}

export function invalidateMessageDetailReadModels(queryClient: QueryClient) {
  void queryClient.invalidateQueries({ queryKey: mailKeys.messageRoot })
  void queryClient.invalidateQueries({ queryKey: mailKeys.conversationRoot })
  void queryClient.invalidateQueries({
    queryKey: mailKeys.conversationSummaryRoot,
  })
}

export function invalidateMailNavigationBootstrapReadModels(
  queryClient: QueryClient,
) {
  void queryClient.invalidateQueries({ queryKey: queryKeys.mailNavigationRead })
  void queryClient.invalidateQueries({ queryKey: queryKeys.tags })
}

export async function invalidateSyncStartedReadModels(
  queryClient: QueryClient,
  accountId: string,
) {
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: queryKeys.mailboxes(accountId) }),
    queryClient.invalidateQueries({ queryKey: queryKeys.smartMailboxes }),
    queryClient.invalidateQueries({ queryKey: queryKeys.tags }),
    queryClient.invalidateQueries({ queryKey: queryKeys.mailNavigationRead }),
    queryClient.invalidateQueries({ queryKey: queryKeys.messagesRoot }),
  ])
}

export async function invalidateComposeSendReadModels(
  queryClient: QueryClient,
  accountId: string,
) {
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: queryKeys.mailboxes(accountId) }),
    queryClient.invalidateQueries({ queryKey: queryKeys.smartMailboxes }),
    queryClient.invalidateQueries({ queryKey: queryKeys.tags }),
    queryClient.invalidateQueries({ queryKey: queryKeys.mailNavigationRead }),
    queryClient.invalidateQueries({ queryKey: queryKeys.senderAddresses }),
    queryClient.invalidateQueries({ queryKey: queryKeys.conversationsRoot }),
  ])
}

export async function invalidateMessageMutationReadModels(
  queryClient: QueryClient,
  target: SourceMessageRef,
) {
  await Promise.all([
    queryClient.invalidateQueries({
      queryKey: queryKeys.mailboxes(target.sourceId),
    }),
    queryClient.invalidateQueries({ queryKey: queryKeys.smartMailboxes }),
    queryClient.invalidateQueries({ queryKey: queryKeys.tags }),
    queryClient.invalidateQueries({ queryKey: queryKeys.mailNavigationRead }),
    queryClient.invalidateQueries({ queryKey: queryKeys.messagesRoot }),
  ])
}

export async function invalidateMessageScopeReadModels(
  queryClient: QueryClient,
  target: SourceMessageRef,
  conversationId: string | null,
) {
  const invalidations = [
    queryClient.invalidateQueries({
      queryKey: mailKeys.message(target.sourceId, target.messageId),
    }),
    queryClient.invalidateQueries({ queryKey: queryKeys.conversationsRoot }),
  ]
  if (conversationId) {
    invalidations.push(
      queryClient.invalidateQueries({
        queryKey: mailKeys.conversation(conversationId),
      }),
      queryClient.invalidateQueries({
        queryKey: mailKeys.conversationSummary(conversationId),
      }),
    )
  }
  await Promise.all(invalidations)
}

export async function invalidateSmartMailboxMutationReadModels(
  queryClient: QueryClient,
  smartMailboxId?: string,
) {
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: queryKeys.messagesRoot }),
    queryClient.invalidateQueries({ queryKey: queryKeys.smartMailboxes }),
    queryClient.invalidateQueries({
      queryKey: smartMailboxId
        ? queryKeys.smartMailbox(smartMailboxId)
        : queryKeys.smartMailboxRoot,
    }),
  ])
}

export function invalidateSmartMailboxReadModels(
  queryClient: QueryClient,
  smartMailboxId?: string | null,
) {
  void queryClient.invalidateQueries({ queryKey: queryKeys.smartMailboxes })
  if (smartMailboxId) {
    void queryClient.invalidateQueries({
      queryKey: queryKeys.smartMailbox(smartMailboxId),
    })
  } else {
    void queryClient.invalidateQueries({ queryKey: queryKeys.smartMailboxRoot })
  }
  invalidateMessageListReadModels(queryClient)
}

export function invalidateMailboxReadModels(
  queryClient: QueryClient,
  accountId: string,
) {
  void queryClient.invalidateQueries({
    queryKey: queryKeys.mailboxes(accountId),
  })
  void queryClient.invalidateQueries({ queryKey: queryKeys.smartMailboxes })
  invalidateMessageListReadModels(queryClient)
}

export function invalidateAccountRuntimeReadModels(
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
  void queryClient.invalidateQueries({ queryKey: queryKeys.mailNavigationRead })
  if (accountId) {
    void queryClient.invalidateQueries({
      queryKey: queryKeys.account(accountId),
    })
    void queryClient.invalidateQueries({
      queryKey: queryKeys.mailboxes(accountId),
    })
  }
  invalidateMessageListReadModels(queryClient)
}

export function invalidateTargetMessageReadModels(
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
