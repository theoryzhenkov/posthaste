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

/** Lucide icon mapping for each known JMAP mailbox role. */
const ROLE_ICON_MAP: Record<KnownMailboxRole, LucideIcon> = {
  inbox: Inbox,
  archive: Archive,
  drafts: PenLine,
  sent: Send,
  junk: ShieldAlert,
  trash: Trash2,
}

/** Type guard for server-provided role strings. */
export function isKnownMailboxRole(
  role: string | null | undefined,
): role is KnownMailboxRole {
  return Boolean(role && role in ROLE_ICON_MAP)
}

/** Heuristically map a mailbox or smart-mailbox name to a known role. */
export function mailboxRoleFromName(name: string): KnownMailboxRole | null {
  switch (name.toLowerCase()) {
    case 'inbox':
      return 'inbox'
    case 'archive':
      return 'archive'
    case 'drafts':
      return 'drafts'
    case 'sent':
      return 'sent'
    case 'junk':
    case 'spam':
      return 'junk'
    case 'trash':
      return 'trash'
    default:
      return null
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

/** Choose a fallback icon for smart mailboxes ("All Mail" gets a Mail icon). */
export function smartMailboxFallbackIcon(name: string): LucideIcon {
  return name.toLowerCase() === 'all mail' ? Mail : Folder
}
