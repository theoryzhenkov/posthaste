import type { QueryClient } from '@tanstack/react-query'

import type { ConversationSummary, ConversationView } from '../api/types'
import { mailKeys } from './keys'

/**
 * Derive a conversation summary from a full conversation view.
 * @spec docs/L1-sync#conversation-pagination
 */
export function summarizeConversation(
  conversation: ConversationView,
): ConversationSummary {
  const latestMessage = conversation.messages[conversation.messages.length - 1]
  return {
    id: conversation.id,
    subject: conversation.subject ?? latestMessage?.subject ?? null,
    preview: latestMessage?.preview ?? null,
    fromName: latestMessage?.fromName ?? null,
    fromEmail: latestMessage?.fromEmail ?? null,
    latestReceivedAt: latestMessage?.receivedAt ?? '',
    unreadCount: conversation.messages.reduce(
      (count, message) => count + (message.isRead ? 0 : 1),
      0,
    ),
    messageCount: conversation.messages.length,
    sourceIds: [
      ...new Set(conversation.messages.map((message) => message.sourceId)),
    ],
    sourceNames: [
      ...new Set(conversation.messages.map((message) => message.sourceName)),
    ],
    latestMessage: latestMessage
      ? { messageId: latestMessage.id, sourceId: latestMessage.sourceId }
      : { messageId: '', sourceId: '' },
    latestSourceName: latestMessage?.sourceName ?? '',
    hasAttachment: conversation.messages.some(
      (message) => message.hasAttachment,
    ),
    isFlagged: conversation.messages.some((message) => message.isFlagged),
  }
}

/**
 * Write a full conversation view into the cache and update the derived summary.
 * @spec docs/L1-ui#data-fetching
 */
export function mergeConversationView(
  queryClient: QueryClient,
  conversation: ConversationView,
) {
  queryClient.setQueryData(mailKeys.conversation(conversation.id), conversation)
  queryClient.setQueryData(
    mailKeys.conversationSummary(conversation.id),
    summarizeConversation(conversation),
  )
}
