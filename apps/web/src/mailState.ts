/**
 * Client-side React Query cache helpers for conversations, messages, and keyword mutations.
 *
 * This module owns the cache key schema, optimistic update logic,
 * local-echo suppression, and conversation-summary derivation.
 *
 * @spec docs/L1-ui#data-fetching
 */
export { mailKeys } from './mail-state/keys'
export {
  getConversationSummary,
  mergeConversationView,
  normalizeConversationPage,
  readConversationIds,
  upsertConversationSummaries,
} from './mail-state/conversations'
export { applyKeywordEventPatch } from './mail-state/keywordEvents'
export {
  applyKeywordPatch,
  deriveKeywordState,
  mergeMessageDetail,
} from './mail-state/keywords'
export {
  recordLocalMutationEvents,
  shouldSuppressLocalEcho,
} from './mail-state/localEcho'
export { findConversationIdForMessage } from './mail-state/lookup'
export { applyMailboxPatch } from './mail-state/mailboxes'
export {
  captureMutableState,
  diffMutableState,
} from './mail-state/mutableState'
export { restoreSnapshots } from './mail-state/snapshots'
export type {
  CachePatchResult,
  ConversationPageSlice,
  KeywordPatch,
  KeywordState,
  MailSelection,
  MailViewSelection,
  MutableState,
  QuerySnapshot,
} from './mail-state/types'
