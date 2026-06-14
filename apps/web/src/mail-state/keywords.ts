import type { InfiniteData, QueryClient } from '@tanstack/react-query'

import type {
  ConversationSummary,
  ConversationView,
  MessageDetail,
  MessagePage,
  MessageSummary,
  SourceMessageRef,
} from '../api/types'
import { queryKeys } from '../queryKeys'
import {
  getConversationSummary,
  summarizeConversation,
} from './conversations'
import { mailKeys } from './keys'
import { snapshotQuery } from './snapshots'
import type {
  CachePatchResult,
  KeywordPatch,
  KeywordState,
  MailSelection,
  QuerySnapshot,
  ReconcileOptions,
} from './types'

/** Derive boolean flags (`isRead`, `isFlagged`) from raw keyword strings. */
export function deriveKeywordState(keywords: string[]): KeywordState {
  return {
    isFlagged: keywords.includes('$flagged'),
    isRead: keywords.includes('$seen'),
    keywords,
  }
}

export function replaceMessageKeywords<T extends MessageSummary | MessageDetail>(
  message: T,
  keywordState: KeywordState,
): T {
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
 * Heuristically patch a conversation summary for a single-message keyword change
 * when the full conversation view is not cached.
 */
function applyHeuristicConversationPatch(
  conversation: ConversationSummary,
  patch: KeywordPatch,
  options?: ReconcileOptions,
): { conversation: ConversationSummary; incomplete: boolean } {
  let incomplete = false
  let nextConversation = conversation

  if (patch.previous.isRead !== patch.next.isRead) {
    const unreadDelta = patch.next.isRead ? -1 : 1
    nextConversation = {
      ...nextConversation,
      unreadCount: Math.max(0, nextConversation.unreadCount + unreadDelta),
    }
  }

  if (patch.previous.isFlagged !== patch.next.isFlagged) {
    if (patch.next.isFlagged) {
      nextConversation = { ...nextConversation, isFlagged: true }
    } else if (
      options?.allowHeuristicFlagClear ||
      nextConversation.messageCount <= 1
    ) {
      nextConversation = { ...nextConversation, isFlagged: false }
    } else {
      incomplete = true
    }
  }

  return { conversation: nextConversation, incomplete }
}

/**
 * Apply a keyword patch to a full conversation view and derive the updated summary.
 */
function applyPatchToConversationView(
  conversation: ConversationView,
  target: MailSelection,
  patch: KeywordPatch,
): {
  changed: boolean
  conversation: ConversationView
  summary: ConversationSummary
} {
  let changed = false
  const messages = conversation.messages.map((message) => {
    if (
      message.sourceId !== target.sourceId ||
      message.id !== target.messageId
    ) {
      return message
    }
    changed = true
    return replaceMessageKeywords(message, patch.next)
  })

  const nextConversation = changed
    ? { ...conversation, messages }
    : conversation
  return {
    changed,
    conversation: nextConversation,
    summary: summarizeConversation(nextConversation),
  }
}

/**
 * Optimistically apply a keyword patch across message, conversation, and summary cache entries.
 * Returns rollback snapshots and whether the patch was incomplete (needs server confirmation).
 * @spec docs/L1-ui#data-fetching
 */
export function applyKeywordPatch(
  queryClient: QueryClient,
  target: MailSelection,
  patch: KeywordPatch,
  options?: ReconcileOptions,
): CachePatchResult {
  const snapshots = [
    snapshotQuery(
      queryClient,
      mailKeys.message(target.sourceId, target.messageId),
    ),
    snapshotQuery(queryClient, mailKeys.conversation(target.conversationId)),
    snapshotQuery(
      queryClient,
      mailKeys.conversationSummary(target.conversationId),
    ),
  ]
  snapshots.push(...patchMessageListQueries(queryClient, target, patch.next))

  let incomplete = false
  let exactSummary: ConversationSummary | null = null

  const messageKey = mailKeys.message(target.sourceId, target.messageId)
  const currentMessage = queryClient.getQueryData<MessageDetail>(messageKey)
  if (currentMessage) {
    queryClient.setQueryData<MessageDetail>(
      messageKey,
      replaceMessageKeywords(currentMessage, patch.next),
    )
  } else {
    incomplete = true
  }

  const conversationKey = mailKeys.conversation(target.conversationId)
  const currentConversation =
    queryClient.getQueryData<ConversationView>(conversationKey)
  if (currentConversation) {
    const updatedConversation = applyPatchToConversationView(
      currentConversation,
      target,
      patch,
    )
    queryClient.setQueryData(conversationKey, updatedConversation.conversation)
    exactSummary = updatedConversation.summary
  }

  const currentSummary = getConversationSummary(
    queryClient,
    target.conversationId,
  )
  if (exactSummary) {
    queryClient.setQueryData(
      mailKeys.conversationSummary(target.conversationId),
      currentSummary ? { ...currentSummary, ...exactSummary } : exactSummary,
    )
  } else if (currentSummary) {
    const heuristicResult = applyHeuristicConversationPatch(
      currentSummary,
      patch,
      options,
    )
    queryClient.setQueryData(
      mailKeys.conversationSummary(target.conversationId),
      heuristicResult.conversation,
    )
    incomplete ||= heuristicResult.incomplete
  } else {
    incomplete = true
  }

  return { incomplete, snapshots }
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

