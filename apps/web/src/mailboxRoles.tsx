/**
 * Mailbox role icons and name-to-role mapping for sidebar and list rendering.
 * @spec docs/L1-ui#component-hierarchy
 */
import {
  Archive,
  Folder,
  Inbox,
  Mail,
  PenLine,
  Send,
  ShieldAlert,
  Trash2,
  type LucideIcon,
} from 'lucide-react'
import type { KnownMailboxRole } from './api/types'
import { ALL_MAIL_DEFAULT_KEY, MAILBOX_ROLES } from './domainVocabulary'

/** Lucide icon mapping for each known JMAP mailbox role. */
const ROLE_ICON_MAP: Record<KnownMailboxRole, LucideIcon> = {
  [MAILBOX_ROLES.Inbox]: Inbox,
  [MAILBOX_ROLES.Archive]: Archive,
  [MAILBOX_ROLES.Drafts]: PenLine,
  [MAILBOX_ROLES.Sent]: Send,
  [MAILBOX_ROLES.Junk]: ShieldAlert,
  [MAILBOX_ROLES.Trash]: Trash2,
}

const MAILBOX_ROLE_ACCENTS: Record<KnownMailboxRole, string> = {
  [MAILBOX_ROLES.Inbox]: '#2B7EC2',
  [MAILBOX_ROLES.Archive]: '#3D8B6D',
  [MAILBOX_ROLES.Drafts]: '#8B5CF6',
  [MAILBOX_ROLES.Sent]: '#D96A42',
  [MAILBOX_ROLES.Junk]: '#C5A100',
  [MAILBOX_ROLES.Trash]: '#8A5B4B',
}

const SMART_MAILBOX_ACCENTS = {
  blue: 'oklch(0.65 0.13 245)',
  coral: 'oklch(0.68 0.17 45)',
  sage: 'oklch(0.68 0.08 145)',
  amber: 'oklch(0.78 0.13 78)',
  violet: 'oklch(0.65 0.13 295)',
  rose: 'oklch(0.70 0.15 12)',
  muted: 'oklch(0.60 0.008 70)',
} as const

/** Type guard for server-provided role strings. */
export function isKnownMailboxRole(
  role: string | null | undefined,
): role is KnownMailboxRole {
  return Boolean(role && role in ROLE_ICON_MAP)
}

/** Accent for source mailbox rows keyed by canonical role. */
export function mailboxRoleAccent(role: string | null): string {
  return isKnownMailboxRole(role) ? MAILBOX_ROLE_ACCENTS[role] : '#7E8691'
}

/** Accent for smart mailbox and tag rows. Role-tagged smart mailboxes key
 *  off the stable `role` (rename/locale-safe); role-less ones (All Mail, user
 *  smart mailboxes, tags) carry no stable id and fall back to the display name. */
export function smartMailboxAccent(
  role: string | null,
  name: string,
): string {
  if (isKnownMailboxRole(role)) {
    switch (role) {
      case MAILBOX_ROLES.Inbox:
      case MAILBOX_ROLES.Archive:
        return SMART_MAILBOX_ACCENTS.blue
      case MAILBOX_ROLES.Drafts:
        return SMART_MAILBOX_ACCENTS.violet
      case MAILBOX_ROLES.Sent:
        return SMART_MAILBOX_ACCENTS.coral
      case MAILBOX_ROLES.Junk:
        return SMART_MAILBOX_ACCENTS.amber
      case MAILBOX_ROLES.Trash:
        return SMART_MAILBOX_ACCENTS.rose
      default:
        return SMART_MAILBOX_ACCENTS.muted
    }
  }
  switch (name.trim().toLowerCase()) {
    case 'inbox':
    case 'all inboxes':
    case 'all mail':
    case 'today':
    case 'archive':
    case 'work':
      return SMART_MAILBOX_ACCENTS.blue
    case 'flagged':
    case 'relevant':
    case 'sent':
    case 'follow-up':
      return SMART_MAILBOX_ACCENTS.coral
    case 'read later':
    case 'read-later':
    case 'junk':
    case 'spam':
      return SMART_MAILBOX_ACCENTS.amber
    case 'bills':
    case 'billing':
    case 'drafts':
      return SMART_MAILBOX_ACCENTS.violet
    case 'newsletters':
    case 'personal':
      return SMART_MAILBOX_ACCENTS.sage
    case 'trash':
      return SMART_MAILBOX_ACCENTS.rose
    default:
      return SMART_MAILBOX_ACCENTS.muted
  }
}

/** Render the Lucide icon for a mailbox role, falling back to a generic folder icon. */
export function renderMailboxRoleIcon(
  role: string | null,
  size = 14,
  fallback: LucideIcon = Folder,
): React.ReactNode {
  const Icon = isKnownMailboxRole(role) ? ROLE_ICON_MAP[role] : fallback
  return <Icon size={size} className="shrink-0" />
}

/** Choose a fallback icon for smart mailboxes: All Mail gets the Mail icon,
 *  keyed off the stable `defaultKey` (rename-safe), not the display name. */
export function smartMailboxFallbackIcon(defaultKey: string | null): LucideIcon {
  return defaultKey === ALL_MAIL_DEFAULT_KEY ? Mail : Folder
}
