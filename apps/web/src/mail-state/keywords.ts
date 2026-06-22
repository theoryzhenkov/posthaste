import type { InfiniteData, QueryClient } from '@tanstack/react-query'

import type {
  ConversationView,
  MessageDetail,
  MessagePage,
  MessageSummary,
  SourceMessageRef,
} from '../api/types'
import { queryKeys } from '../queryKeys'
import { getConversationSummary, summarizeConversation } from './conversations'
import { mailKeys } from './keys'
import { snapshotQuery } from './snapshots'
import type { KeywordState, QuerySnapshot } from './types'

/** Derive boolean flags (`isRead`, `isFlagged`) from raw keyword strings. */
export function deriveKeywordState(keywords: string[]): KeywordState {
  return {
    isFlagged: keywords.includes('$flagged'),
    isRead: keywords.includes('$seen'),
    keywords,
  }
}

export function replaceMessageKeywords<
  T extends MessageSummary | MessageDetail,
>(message: T, keywordState: KeywordState): T {
  return {
    ...message,
    isFlagged: keywordState.isFlagged,
    isRead: keywordState.isRead,
    keywords: keywordState.keywords,
  }
}

export function patchMessageListQueries(
  queryClient: QueryClient,
  target: SourceMessageRef,
  keywordState: KeywordState,
): QuerySnapshot[] {
  const snapshots: QuerySnapshot[] = []
  for (const [queryKey, data] of queryClient.getQueriesData<
    InfiniteData<MessagePage, string | null>
  >({
    queryKey: queryKeys.messagesRoot,
  })) {
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

    snapshots.push(snapshotQuery(queryClient, queryKey))
    queryClient.setQueryData<InfiniteData<MessagePage, string | null>>(
      queryKey,
      {
        ...data,
        pages: data.pages.map((page) => ({
          ...page,
          items: page.items.map((message) =>
            message.sourceId === target.sourceId &&
            message.id === target.messageId
              ? replaceMessageKeywords(message, keywordState)
              : message,
          ),
        })),
      },
    )
  }
  return snapshots
}

/**
 * Merge a fresh message detail into the cache and update the parent conversation summary.
 * @spec docs/L1-ui#data-fetching
 */
export function mergeMessageDetail(
  queryClient: QueryClient,
  detail: MessageDetail,
  conversationId: string,
) {
  queryClient.setQueryData(mailKeys.message(detail.sourceId, detail.id), detail)
  patchMessageListQueries(
    queryClient,
    { messageId: detail.id, sourceId: detail.sourceId },
    deriveKeywordState(detail.keywords),
  )

  const conversationKey = mailKeys.conversation(conversationId)
  const conversation =
    queryClient.getQueryData<ConversationView>(conversationKey)
  if (!conversation) {
    return false
  }

  const messages = conversation.messages.map((message) =>
    message.sourceId === detail.sourceId && message.id === detail.id
      ? replaceMessageKeywords(message, detail)
      : message,
  )
  const nextConversation = { ...conversation, messages }
  queryClient.setQueryData(conversationKey, nextConversation)

  const summary = summarizeConversation(nextConversation)
  const currentSummary = getConversationSummary(queryClient, conversationId)
  queryClient.setQueryData(
    mailKeys.conversationSummary(conversationId),
    currentSummary ? { ...currentSummary, ...summary } : summary,
  )

  return true
}
