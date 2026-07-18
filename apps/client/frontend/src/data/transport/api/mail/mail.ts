// Mail wire types. The projections the backend answers with are the
// generated twins in `@/gen`; this module re-exports them under their
// historical names so the whole tree shares one type identity. The remaining
// local shapes are client-side compositions (pages, conversation rollups)
// built from those projections.

export type {
  ListUnsubscribe,
  MessageAttachment,
  MessageSortField,
  MessageSummary,
  TagSummary,
} from '@/gen'

/** One mailbox with its counters (generated `MailboxSummary`). */
export type { MailboxSummary as Mailbox } from '@/gen'

import type { MessageAttachment, MessageSummary, ListUnsubscribe } from '@/gen'

export type KnownMailboxRole =
  | 'inbox'
  | 'archive'
  | 'drafts'
  | 'sent'
  | 'junk'
  | 'trash'
  | 'snooze'

/**
 * Body for creating a new top-level mailbox. Flat create — a name only.
 */
export interface CreateMailboxInput {
  name: string
}

/**
 * Options for deleting a mailbox. `removeEmails` is the confirm-with-count
 * safety flag: a non-empty mailbox delete is refused unless it is `true`.
 */
export interface DeleteMailboxInput {
  removeEmails: boolean
}

/** One window of a mail list: rows plus the continuation cursor. */
export interface MessagePage {
  items: MessageSummary[]
  nextCursor: string | null
}

/**
 * Full message detail including sanitized body HTML — the summary projection
 * flattened together with the read-time fields of `MessageDetailResult`.
 */
export interface MessageDetail extends MessageSummary {
  bodyHtml: string | null
  bodyText: string | null
  attachments: MessageAttachment[]
  /** RFC 2369/8058 unsubscribe targets, when the message carries valid ones. */
  listUnsubscribe?: ListUnsubscribe | null
}

/**
 * Pair that uniquely identifies a message within a source account.
 */
export interface SourceMessageRef {
  sourceId: string
  messageId: string
}

/**
 * Locally derived conversation summary for middle-pane rows.
 */
export interface ConversationSummary {
  id: string
  subject: string | null
  preview: string | null
  fromName: string | null
  fromEmail: string | null
  latestReceivedAt: string
  unreadCount: number
  messageCount: number
  sourceIds: string[]
  sourceNames: string[]
  latestMessage: SourceMessageRef
  latestSourceName: string
  hasAttachment: boolean
  isFlagged: boolean
}

/**
 * Cursor-paginated conversation response.
 */
export interface ConversationPage {
  items: ConversationSummary[]
  nextCursor: string | null
}

/**
 * Full conversation view with all message summaries in the thread.
 */
export interface ConversationView {
  id: string
  subject: string | null
  messages: MessageSummary[]
}
