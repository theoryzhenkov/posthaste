import type { InfiniteData, QueryClient, QueryKey } from '@tanstack/react-query'

import type {
  ConversationView,
  MessageDetail,
  MessagePage,
  SourceMessageRef,
} from '../api/types'
import { queryKeys } from '../queryKeys'
import { summarizeConversation } from './conversations'
import { mailKeys } from './keys'
import { snapshotQuery } from './snapshots'
import type { CachePatchResult, MailSelection, QuerySnapshot } from './types'

/**
 * Decide whether a message with `nextMailboxIds` still belongs in the list view
 * identified by `queryKey`. Source-mailbox views are decidable from membership;
 * smart-mailbox and unscoped views are not (returns null → caller marks the
 * patch incomplete and falls back to invalidation).
 */
function messageBelongsToListView(
  queryKey: QueryKey,
  nextMailboxIds: string[],
): boolean | null {
  const selection = queryKey[1] as
    | { kind: 'source-mailbox'; sourceId: string; mailboxId: string }
    | { kind: 'smart-mailbox'; id: string }
    | null
    | undefined
  if (!selection) {
    return null
  }
  if (selection.kind === 'source-mailbox') {
    return nextMailboxIds.includes(selection.mailboxId)
  }
  return null
}

function patchMessageListMembership(
  queryClient: QueryClient,
  target: SourceMessageRef,
  nextMailboxIds: string[],
  destroy: boolean,
): { incomplete: boolean; snapshots: QuerySnapshot[] } {
  const snapshots: QuerySnapshot[] = []
  let incomplete = false
  for (const [queryKey, data] of queryClient.getQueriesData<
    InfiniteData<MessagePage, string | null>
  >({ queryKey: queryKeys.messagesRoot })) {
    const hasTarget = data?.pages.some((page) =>
      page.items.some(
        (message) =>
          message.sourceId === target.sourceId &&
          message.id === target.messageId,
      ),
    )
    if (!data || !hasTarget) {
      continue
    }

    const belongs = destroy
      ? false
      : messageBelongsToListView(queryKey, nextMailboxIds)
    if (belongs === null) {
      // Membership cannot be decided from the cache (smart mailbox / unscoped):
      // leave the row in place and let server reconciliation settle it.
      incomplete = true
    }

    snapshots.push(snapshotQuery(queryClient, queryKey))
    queryClient.setQueryData<InfiniteData<MessagePage, string | null>>(
      queryKey,
      {
        ...data,
        pages: data.pages.map((page) => ({
          ...page,
          items:
            belongs === false
              ? page.items.filter(
                  (message) =>
                    !(
                      message.sourceId === target.sourceId &&
                      message.id === target.messageId
                    ),
                )
              : page.items.map((message) =>
                  message.sourceId === target.sourceId &&
                  message.id === target.messageId
                    ? { ...message, mailboxIds: nextMailboxIds }
                    : message,
                ),
        })),
      },
    )
  }
  return { incomplete, snapshots }
}

/**
 * Optimistically apply a mailbox-membership change (move/archive/trash) or a
 * destroy across message detail, conversation view + summary, and list views.
 * Mirrors {@link applyKeywordPatch}: returns rollback snapshots and whether the
 * patch was incomplete and needs server confirmation.
 * @spec docs/L1-ui#data-fetching
 */
export function applyMailboxPatch(
  queryClient: QueryClient,
  target: MailSelection,
  nextMailboxIds: string[],
  options?: { destroy?: boolean },
): CachePatchResult {
  const destroy = options?.destroy ?? false
  const snapshots: QuerySnapshot[] = []
  let incomplete = false

  const messageKey = mailKeys.message(target.sourceId, target.messageId)
  const currentMessage = queryClient.getQueryData<MessageDetail>(messageKey)
  if (currentMessage) {
    snapshots.push(snapshotQuery(queryClient, messageKey))
    if (destroy) {
      queryClient.removeQueries({ queryKey: messageKey, exact: true })
    } else {
      queryClient.setQueryData<MessageDetail>(messageKey, {
        ...currentMessage,
        mailboxIds: nextMailboxIds,
      })
    }
  }

  const conversationKey = mailKeys.conversation(target.conversationId)
  const conversation =
    queryClient.getQueryData<ConversationView>(conversationKey)
  if (conversation) {
    const messages = destroy
      ? conversation.messages.filter(
          (message) =>
            !(
              message.sourceId === target.sourceId &&
              message.id === target.messageId
            ),
        )
      : conversation.messages.map((message) =>
          message.sourceId === target.sourceId &&
          message.id === target.messageId
            ? { ...message, mailboxIds: nextMailboxIds }
            : message,
        )
    snapshots.push(snapshotQuery(queryClient, conversationKey))
    queryClient.setQueryData(conversationKey, { ...conversation, messages })

    const summaryKey = mailKeys.conversationSummary(target.conversationId)
    snapshots.push(snapshotQuery(queryClient, summaryKey))
    if (messages.length === 0) {
      queryClient.removeQueries({ queryKey: summaryKey, exact: true })
    } else {
      queryClient.setQueryData(
        summaryKey,
        summarizeConversation({ ...conversation, messages }),
      )
    }
  }

  const listResult = patchMessageListMembership(
    queryClient,
    target,
    nextMailboxIds,
    destroy,
  )
  snapshots.push(...listResult.snapshots)
  incomplete ||= listResult.incomplete

  return { incomplete, snapshots }
}
