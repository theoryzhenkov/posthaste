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
  // `messagesRoot` (the mail list ROWS) is owned by the entity store — it
  // drives rows via SSE-fed frames, so it is not REST-invalidated here. Counts
  // (mailboxes/smart-mailboxes) are react-query state: the sync's
  // `message.updated` events invalidate them (debounced) as changes land, so
  // sync START only refreshes the navigation read models.
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: queryKeys.smartMailboxes }),
    queryClient.invalidateQueries({ queryKey: queryKeys.tags }),
    queryClient.invalidateQueries({ queryKey: queryKeys.mailNavigationRead }),
  ])
}

export async function invalidateComposeSendReadModels(
  queryClient: QueryClient,
) {
  // A send moves drafts/sent counts, so the mailbox count read models refetch
  // too (all accounts — the send path has no account id here; the root prefix
  // covers every `mailboxes(accountId)` key).
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: ['mailboxes'] }),
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
  // `mailboxes(accountId)` carries the canonical counts + structure
  // (RFC-L2-count-unification): mailbox changes ALWAYS refetch it — there is
  // no store-owned count carve-out anymore. `skipStoreOwned` now governs only
  // the mail-list ROWS (entity-store-owned when active).
  void queryClient.invalidateQueries({
    queryKey: queryKeys.mailboxes(accountId),
  })
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
    // `mailboxes(accountId)` carries the canonical counts
    // (RFC-L2-count-unification) — refetch them on account-level changes
    // (sync completion routes through here, one of the count triggers).
    void queryClient.invalidateQueries({
      queryKey: queryKeys.mailboxes(accountId),
    })
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
