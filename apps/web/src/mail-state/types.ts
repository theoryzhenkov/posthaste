import type { QueryKey } from '@tanstack/react-query'
import type { MessageSummary, SourceMessageRef } from '../api/types'

/**
 * Selected message reference used by list and detail views.
 * @spec docs/L1-ui#messagelist
 */
export type MailSelection = SourceMessageRef & { conversationId: string }

/**
 * Current sidebar selection -- either a smart mailbox or a source+mailbox pair.
 * @spec docs/L0-ui#navigation-model
 */
export type MailViewSelection =
  | { kind: 'smart-mailbox'; id: string }
  | { kind: 'source-mailbox'; sourceId: string; mailboxId: string }
  | null

/**
 * Normalized conversation page stored in the infinite query cache.
 * Summaries are extracted into per-ID cache entries; only IDs remain here.
 * @spec docs/L1-api#cursor-pagination
 */
export type ConversationPageSlice = {
  itemIds: string[]
  nextCursor: string | null
}

/** Snapshot of a single query entry for optimistic rollback. */
export type QuerySnapshot = {
  data: unknown
  existed: boolean
  queryKey: QueryKey
}

/** Derived boolean flags from raw JMAP keyword strings. */
export type KeywordState = Pick<
  MessageSummary,
  'isFlagged' | 'isRead' | 'keywords'
>
