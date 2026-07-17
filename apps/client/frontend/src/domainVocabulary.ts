/**
 * Typed frontend mirror of serialized domain vocabulary strings.
 *
 */
import type { KnownMailboxRole } from './api/types'

export type { KnownMailboxRole } from './api/types'

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

// Event topics come from the generated `src/api/events.gen.ts`: the topic union,
// named accessors (`EVENT_TOPICS`), and `isEventTopic` guard are codegen'd from
// `asyncapi.json` and drift-checked, so an unhandled server-side topic is a
// client compile error.
