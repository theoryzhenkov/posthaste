import type { InfiniteData, QueryClient } from '@tanstack/react-query'

import type {
  ConversationSummary,
  ConversationView,
  MessageDetail,
  MessagePage,
  SourceMessageRef,
} from '../api/types'
import { queryKeys } from '../queryKeys'
import { mailKeys } from './keys'

/**
 * Look up a conversation ID for a message by checking cached detail,
 * conversation views, and conversation summaries.
 */
export function findConversationIdForMessage(
  queryClient: QueryClient,
  target: SourceMessageRef,
): string | null {
  const cachedMessage = queryClient.getQueryData<MessageDetail>(
    mailKeys.message(target.sourceId, target.messageId),
  )
  if (cachedMessage) {
    return cachedMessage.conversationId
  }

  for (const [, data] of queryClient.getQueriesData<
    InfiniteData<MessagePage, string | null>
  >({ queryKey: queryKeys.messagesRoot })) {
    const match = data?.pages
      .flatMap((page) => page.items)
      .find(
        (message) =>
          message.sourceId === target.sourceId &&
          message.id === target.messageId,
      )
    if (match) {
      return match.conversationId
    }
  }

  for (const [, conversation] of queryClient.getQueriesData<ConversationView>({
    queryKey: mailKeys.conversationRoot,
  })) {
    if (
      conversation?.messages.some(
        (message) =>
          message.sourceId === target.sourceId &&
          message.id === target.messageId,
      )
    ) {
      return conversation.id
    }
  }

  for (const [, summary] of queryClient.getQueriesData<ConversationSummary>({
    queryKey: mailKeys.conversationSummaryRoot,
  })) {
    if (
      summary?.latestMessage.sourceId === target.sourceId &&
      summary.latestMessage.messageId === target.messageId
    ) {
      return summary.id
    }
  }

  return null
}
