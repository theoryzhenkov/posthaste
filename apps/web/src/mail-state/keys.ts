import { queryKeys } from '../queryKeys'

/**
 * Canonical React Query key builders for mail-related data.
 * @spec docs/L1-ui#data-fetching
 */
export const mailKeys = {
  messageRoot: queryKeys.messageDetailsRoot,
  conversationRoot: queryKeys.conversationDetailsRoot,
  conversationSummaryRoot: queryKeys.conversationSummariesRoot,
  message: (sourceId: string, messageId: string) =>
    ['message', sourceId, messageId] as const,
  conversation: (conversationId: string) =>
    ['conversation', conversationId] as const,
  conversationSummary: (conversationId: string) =>
    ['conversation-summary', conversationId] as const,
}
