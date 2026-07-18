/**
 * Typed frontend mirror of serialized domain vocabulary strings.
 *
 */
import type { KnownMailboxRole } from '../data/transport/api/index'

export type { KnownMailboxRole } from '../data/transport/api/index'

export const MAILBOX_ROLES = {
  Inbox: 'inbox',
  Archive: 'archive',
  Drafts: 'drafts',
  Sent: 'sent',
  Junk: 'junk',
  Trash: 'trash',
  Snooze: 'snooze',
} as const satisfies Record<string, KnownMailboxRole>

export const KNOWN_MAILBOX_ROLES = [
  MAILBOX_ROLES.Inbox,
  MAILBOX_ROLES.Archive,
  MAILBOX_ROLES.Drafts,
  MAILBOX_ROLES.Sent,
  MAILBOX_ROLES.Junk,
  MAILBOX_ROLES.Trash,
  MAILBOX_ROLES.Snooze,
] as const satisfies readonly KnownMailboxRole[]

/** Roles a user can assign to a smart mailbox — the provider roles, excluding
 *  the system-managed `snooze`. Mirrors the backend's accepted set. */
export const ASSIGNABLE_MAILBOX_ROLES = [
  MAILBOX_ROLES.Inbox,
  MAILBOX_ROLES.Archive,
  MAILBOX_ROLES.Drafts,
  MAILBOX_ROLES.Sent,
  MAILBOX_ROLES.Junk,
  MAILBOX_ROLES.Trash,
] as const satisfies readonly KnownMailboxRole[]

/** The `defaultKey` of the built-in All Mail smart mailbox (empty rule,
 *  matches every message). Shared so the predicate resolver + any presenter
 *  agree on the single key. */
export const ALL_MAIL_DEFAULT_KEY = 'all-mail'

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

// Event topics come from the generated protocol types in `src/gen/` (ts-rs
// output of the models crate: `DomainEventKind`, the payload shapes), not
// from this file. Below are the CLIENT-ONLY vocabularies: closed string sets
// minted on this side of the wire, each owned here once.

/** Lifecycle of one mirror entry, as rendered by the data hooks. */
const QUERY_STATUS = {
  Loading: 'loading',
  Ready: 'ready',
  Stale: 'stale',
  Error: 'error',
} as const

export type QueryStatus = (typeof QUERY_STATUS)[keyof typeof QUERY_STATUS]

/** Stream/connection state the data facade exposes to the UI. */
const CONNECTION_STATUS = {
  Connected: 'connected',
  Reconnecting: 'reconnecting',
  Stale: 'stale',
} as const

export type ConnectionStatus =
  (typeof CONNECTION_STATUS)[keyof typeof CONNECTION_STATUS]

/** Severity of an entry in the app-wide notification center. */
const NOTIFICATION_SEVERITY = {
  Error: 'error',
  Warning: 'warning',
  Info: 'info',
} as const

export type NotificationSeverity =
  (typeof NOTIFICATION_SEVERITY)[keyof typeof NOTIFICATION_SEVERITY]

/** Severity of an account's derived health notice (`ok` = no notice). */
const ACCOUNT_HEALTH_SEVERITY = {
  Ok: 'ok',
  Info: 'info',
  Warn: 'warn',
  Error: 'error',
} as const

export type AccountHealthSeverity =
  (typeof ACCOUNT_HEALTH_SEVERITY)[keyof typeof ACCOUNT_HEALTH_SEVERITY]

/** How the message list groups rows. */
const MESSAGE_LIST_VIEW_MODE = {
  Messages: 'messages',
  Conversations: 'conversations',
} as const

export type MessageListViewMode =
  (typeof MESSAGE_LIST_VIEW_MODE)[keyof typeof MESSAGE_LIST_VIEW_MODE]

/** The keyboard-navigable regions of the mail shell. The detail pane is NOT
 *  focusable — it only displays the list's selected message, and `j`/`k` in
 *  the list drive it. */
export const PANE_ID = {
  Sidebar: 'sidebar',
  List: 'list',
} as const

export type PaneId = (typeof PANE_ID)[keyof typeof PANE_ID]

/** Client-side sort direction for the message-list columns (the wire carries
 *  a `descending` boolean; this is the UI's two-way vocabulary). */
const SORT_DIRECTION = {
  Asc: 'asc',
  Desc: 'desc',
} as const

export type SortDirection =
  (typeof SORT_DIRECTION)[keyof typeof SORT_DIRECTION]
