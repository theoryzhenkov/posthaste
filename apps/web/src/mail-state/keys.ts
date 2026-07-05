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
  // The message BODY is a separate lazy resource (`GET .../body`) — the detail
  // read surface serves headers-only (`get_message_detail_without_body`). Cache
  // the fetched body here so it survives the detail pane unmounting, letting the
  // reply composer seed its `>`-quote from the body the user just read.
  messageBody: (sourceId: string, messageId: string) =>
    ['message-body', sourceId, messageId] as const,
  conversation: (conversationId: string) =>
    ['conversation', conversationId] as const,
  conversationSummary: (conversationId: string) =>
    ['conversation-summary', conversationId] as const,
}
