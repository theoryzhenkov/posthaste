/**
 * Typed frontend mirror of serialized domain vocabulary strings.
 *
 * @spec docs/L1-api#mailbox-metadata
 * @spec docs/L1-api#navigation
 * @spec docs/L1-api#sse-event-stream
 */
import type { KnownMailboxRole } from './api/types'

export const MAILBOX_ROLES = {
  Inbox: 'inbox',
  Archive: 'archive',
  Drafts: 'drafts',
  Sent: 'sent',
  Junk: 'junk',
  Trash: 'trash',
} as const satisfies Record<string, KnownMailboxRole>

export const KNOWN_MAILBOX_ROLES = [
  MAILBOX_ROLES.Inbox,
  MAILBOX_ROLES.Archive,
  MAILBOX_ROLES.Drafts,
  MAILBOX_ROLES.Sent,
  MAILBOX_ROLES.Junk,
  MAILBOX_ROLES.Trash,
] as const satisfies readonly KnownMailboxRole[]

export const SYSTEM_KEYWORD_PREFIX = '$'

export const SYSTEM_KEYWORDS = {
  Seen: '$seen',
  Draft: '$draft',
  Flagged: '$flagged',
  Answered: '$answered',
  Forwarded: '$forwarded',
} as const

export type SystemKeyword =
  (typeof SYSTEM_KEYWORDS)[keyof typeof SYSTEM_KEYWORDS]

export const KNOWN_SYSTEM_KEYWORDS = [
  SYSTEM_KEYWORDS.Seen,
  SYSTEM_KEYWORDS.Draft,
  SYSTEM_KEYWORDS.Flagged,
  SYSTEM_KEYWORDS.Answered,
  SYSTEM_KEYWORDS.Forwarded,
] as const satisfies readonly SystemKeyword[]

export const EVENT_TOPICS = {
  SyncCompleted: 'sync.completed',
  SyncFailed: 'sync.failed',
  MessageUpdated: 'message.updated',
  MessageKeywordsChanged: 'message.keywords_changed',
  MessageBodyCached: 'message.body_cached',
  MessageMailboxesChanged: 'message.mailboxes_changed',
  MessageArrived: 'message.arrived',
  MailboxUpdated: 'mailbox.updated',
  AccountUpdated: 'account.updated',
  AccountCreated: 'account.created',
  AccountDeleted: 'account.deleted',
  AccountStatusChanged: 'account.status_changed',
  PushConnected: 'push.connected',
  PushDisconnected: 'push.disconnected',
} as const

export type DomainEventTopic = (typeof EVENT_TOPICS)[keyof typeof EVENT_TOPICS]
