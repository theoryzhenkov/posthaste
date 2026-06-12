import type { Recipient } from './compose'

export type KnownMailboxRole =
  | 'inbox'
  | 'archive'
  | 'drafts'
  | 'sent'
  | 'junk'
  | 'trash'

export interface Mailbox {
  id: string
  name: string
  role: string | null
  unreadEmails: number
  totalEmails: number
}

/** @spec docs/L1-api#endpoint-table */
export interface PatchMailboxInput {
  role: KnownMailboxRole | null
}

/**
 * Compact message metadata used in conversation rows and thread switchers.
 * @spec docs/L1-ui#messagelist
 */
export interface MessageSummary {
  id: string
  sourceId: string
  sourceName: string
  sourceThreadId: string
  conversationId: string
  subject: string | null
  fromName: string | null
  fromEmail: string | null
  to: Recipient[]
  preview: string | null
  receivedAt: string
  hasAttachment: boolean
  isRead: boolean
  isFlagged: boolean
  mailboxIds: string[]
  keywords: string[]
}

/** @spec docs/L1-api#cursor-pagination */
export type MessageSortField =
  | 'date'
  | 'from'
  | 'subject'
  | 'source'
  | 'flagged'
  | 'attachment'

/** @spec docs/L1-api#cursor-pagination */
export interface MessagePage {
  items: MessageSummary[]
  nextCursor: string | null
}

/**
 * Reference to a raw message file stored on the backend.
 * @spec docs/L1-sync#body-lazy
 */
export interface RawMessageRef {
  path: string
  sha256: string
  size: number
  mimeType: string
  fetchedAt: string
}

export interface MessageAttachment {
  id: string
  blobId: string
  partId: string | null
  filename: string | null
  mimeType: string
  size: number
  disposition: string | null
  cid: string | null
  isInline: boolean
}

/**
 * Full message detail including sanitized body HTML.
 * @spec docs/L1-api#message-body-sanitization
 */
export interface MessageDetail extends MessageSummary {
  bodyHtml: string | null
  bodyText: string | null
  rawMessage: RawMessageRef | null
  attachments: MessageAttachment[]
}

/**
 * Pair that uniquely identifies a message within a source account.
 * @spec docs/L1-api#endpoint-table
 */
export interface SourceMessageRef {
  sourceId: string
  messageId: string
}

/**
 * Locally derived conversation summary for middle-pane rows.
 * @spec docs/L1-sync#conversation-pagination
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
 * @spec docs/L1-api#cursor-pagination
 */
export interface ConversationPage {
  items: ConversationSummary[]
  nextCursor: string | null
}

/**
 * Full conversation view with all message summaries in the thread.
 * @spec docs/L1-ui#messagedetail-and-emailframe
 */
export interface ConversationView {
  id: string
  subject: string | null
  messages: MessageSummary[]
}

/** @spec docs/L1-api#endpoint-table */
export interface TagSummary {
  name: string
  unreadMessages: number
  totalMessages: number
}
