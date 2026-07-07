import type { Recipient } from './compose'

export type KnownMailboxRole =
  | 'inbox'
  | 'archive'
  | 'drafts'
  | 'sent'
  | 'junk'
  | 'trash'
  | 'snooze'

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
 * Body for creating a new top-level mailbox. Flat create — a name only.
 * @spec docs/eph/RFC-L2-mailbox-management
 */
export interface CreateMailboxInput {
  name: string
}

/**
 * Options for deleting a mailbox. `removeEmails` is the confirm-with-count
 * safety flag: a non-empty mailbox delete is refused with 409 `mailbox_not_empty`
 * unless it is `true`.
 * @spec docs/eph/RFC-L2-mailbox-management
 */
export interface DeleteMailboxInput {
  removeEmails: boolean
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
  /** Per-message authority-state version (IMAP `max(modseq)`); `null` for
   * providers without one (JMAP/mock). Drives the replica's stale-re-serve guard. */
  version?: number | null
  /** RFC822 `Message-ID`; with `inReplyTo` builds the conversation reply tree. */
  rfcMessageId?: string | null
  /** `Message-ID` this is a reply to (parent in the reply tree). */
  inReplyTo?: string | null
  /**
   * Stable `X-Posthaste-Draft-Id` when this list row is a draft we saved
   * (D131); `null`/absent otherwise. Surfaced on the summary so a list-row
   * discard carries the stable id — it survives the provider id rotation a
   * JMAP autosave causes, so the discard never targets a stale Email id.
   */
  draftId?: string | null
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
  /**
   * Snapshot-attach consistency token (RFC-L2-scripting §5.3): the event-log
   * head seq as-of this read, for a gap-free tap tail from that point.
   */
  asOfSeq?: number | null
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
 * Parsed `List-Unsubscribe` targets (RFC 2369) with the RFC 8058 one-click
 * marker, validated at ingest. Carried on the DETAIL only — the affordance
 * renders in the detail header; list summaries never carry it.
 */
export interface ListUnsubscribe {
  /** Validated https target (no userinfo, no IP literal); absent otherwise. */
  https?: string | null
  /** Full `mailto:` URI (query params like `subject=` prefill the composer). */
  mailto?: string | null
  /** True when `List-Unsubscribe-Post: List-Unsubscribe=One-Click` accompanied
   *  an https target — the backend may POST it (after user confirmation). */
  oneClick: boolean
}

/** Response of the one-click unsubscribe endpoint (2xx from the list server). */
export interface UnsubscribeAck {
  httpStatus: number
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
  /** RFC 2369/8058 unsubscribe targets, when the message carries valid ones. */
  listUnsubscribe?: ListUnsubscribe | null
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
  /**
   * Snapshot-attach consistency token (RFC-L2-scripting §5.3): the event-log
   * head seq as-of this read, for a gap-free tap tail from that point.
   */
  asOfSeq?: number | null
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
