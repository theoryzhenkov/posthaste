/**
 * Client-side React Query cache helpers for conversations, messages, and keyword mutations.
 *
 * This module owns the cache key schema, optimistic update logic,
 * local-echo suppression, and conversation-summary derivation.
 *
 * @spec docs/L1-ui#data-fetching
 */
export { mailKeys } from './mail-state/keys'
export { mergeConversationView } from './mail-state/conversations'
export { deriveKeywordState } from './mail-state/keywords'
export { findConversationIdForMessage } from './mail-state/lookup'
export type {
  ConversationPageSlice,
  KeywordState,
  MailSelection,
  MailViewSelection,
  QuerySnapshot,
} from './mail-state/types'
