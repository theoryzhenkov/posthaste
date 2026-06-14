import type { QueryClient } from '@tanstack/react-query'

import type { MessageDetail, SourceMessageRef } from '../api/types'
import { mailKeys } from './keys'
import { applyKeywordPatch, deriveKeywordState } from './keywords'
import { findConversationIdForMessage } from './lookup'

/**
 * Apply a keyword change from an SSE event by resolving the conversation
 * from the cache and delegating to {@link applyKeywordPatch}.
 * @spec docs/L1-ui#live-prepend-behavior
 */
export function applyKeywordEventPatch(
  queryClient: QueryClient,
  target: SourceMessageRef,
  keywords: string[],
): boolean {
  const conversationId = findConversationIdForMessage(queryClient, target)
  if (!conversationId) {
    return false
  }

  const currentMessage = queryClient.getQueryData<MessageDetail>(
    mailKeys.message(target.sourceId, target.messageId),
  )
  if (!currentMessage) {
    return false
  }

  applyKeywordPatch(
    queryClient,
    { ...target, conversationId },
    {
      next: deriveKeywordState(keywords),
      previous: deriveKeywordState(currentMessage.keywords),
    },
  )
  return true
}
