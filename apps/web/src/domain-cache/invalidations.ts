import type { QueryClient } from '@tanstack/react-query'

import type { DomainEvent } from '../api/types'
import { findConversationIdForMessage, mailKeys } from '../mailState'
import { queryKeys } from '../queryKeys'
import { eventTarget, payloadConversationId } from './payload'

export function invalidateMessageListReadModels(
  queryClient: QueryClient,
  options: { skipStoreOwned?: boolean } = {},
) {
  // `messagesRoot` (the mail list) is owned by the entity store when active —
  // it drives rows via synthesized view frames, so invalidating would refetch
  // redundantly. `conversationsRoot` is not store-owned and always invalidates.
  if (!options.skipStoreOwned) {
    void queryClient.invalidateQueries({ queryKey: queryKeys.messagesRoot })
  }
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
) {
  // `messagesRoot` (the mail list) + `mailboxes` (counts) are owned by the
  // entity store — it drives them via SSE-fed frames + `setQueryData`, so they
  // are not REST-invalidated here. The rest are not store-owned.
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: queryKeys.smartMailboxes }),
    queryClient.invalidateQueries({ queryKey: queryKeys.tags }),
    queryClient.invalidateQueries({ queryKey: queryKeys.mailNavigationRead }),
  ])
}

export async function invalidateComposeSendReadModels(
  queryClient: QueryClient,
) {
  // `mailboxes` (counts) is store-owned — not REST-invalidated here. The rest
  // are not (smart-mailbox/tag counts, sender addresses, conversations).
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: queryKeys.smartMailboxes }),
    queryClient.invalidateQueries({ queryKey: queryKeys.tags }),
    queryClient.invalidateQueries({ queryKey: queryKeys.mailNavigationRead }),
    queryClient.invalidateQueries({ queryKey: queryKeys.senderAddresses }),
    queryClient.invalidateQueries({ queryKey: queryKeys.conversationsRoot }),
  ])
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
  options: { skipStoreOwned?: boolean } = {},
) {
  // `mailboxes(accountId)` carries the counts the entity store owns when
  // active (it writes them via `setQueryData`), so invalidating would refetch
  // redundantly. Smart-mailboxes + the message list are not store-owned.
  if (!options.skipStoreOwned) {
    void queryClient.invalidateQueries({
      queryKey: queryKeys.mailboxes(accountId),
    })
  }
  void queryClient.invalidateQueries({ queryKey: queryKeys.smartMailboxes })
  invalidateMessageListReadModels(queryClient, options)
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
    // The compose sender identity derives from account config (`full_name`),
    // so an account edit must refresh it or compose keeps the stale name.
    void queryClient.invalidateQueries({
      queryKey: queryKeys.identity(accountId),
    })
    // `mailboxes(accountId)` (counts) is store-owned — not REST-invalidated.
  }
  // The mail list is store-owned — skip its REST invalidation.
  invalidateMessageListReadModels(queryClient, { skipStoreOwned: true })
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
