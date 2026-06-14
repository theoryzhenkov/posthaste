import { queryKeys } from '../queryKeys'
import type { MailViewSelection } from './types'

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
  view: (
    selection: MailViewSelection,
    sort?: { columnId: string; direction: string },
    q?: string,
  ) => {
    const base = !selection
      ? (['conversations', 'none'] as const)
      : selection.kind === 'smart-mailbox'
        ? (['conversations', 'smart-mailbox', selection.id] as const)
        : ([
            'conversations',
            'source-mailbox',
            selection.sourceId,
            selection.mailboxId,
          ] as const)
    const withSort = sort ? [...base, sort.columnId, sort.direction] : base
    if (q) {
      return [...withSort, 'q', q] as const
    }
    return withSort
  },
}
