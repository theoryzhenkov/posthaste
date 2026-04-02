import { Archive, Folder, Inbox, Mail, PenLine, Send, ShieldAlert, Trash2, type LucideIcon } from "lucide-react";
import type { KnownMailboxRole } from "./api/types";

const ROLE_ICON_MAP: Record<KnownMailboxRole, LucideIcon> = {
  inbox: Inbox,
  archive: Archive,
  drafts: PenLine,
  sent: Send,
  junk: ShieldAlert,
  trash: Trash2,
};

export function mailboxRoleFromName(name: string): KnownMailboxRole | null {
  switch (name.toLowerCase()) {
    case "inbox":
      return "inbox";
    case "archive":
      return "archive";
    case "drafts":
      return "drafts";
    case "sent":
      return "sent";
    case "junk":
    case "spam":
      return "junk";
    case "trash":
      return "trash";
    default:
      return null;
  }
}

export function renderMailboxRoleIcon(
  role: KnownMailboxRole | null,
  size = 14,
  fallback: LucideIcon = Folder,
): React.ReactNode {
  const Icon = role ? ROLE_ICON_MAP[role] : fallback;
  return <Icon size={size} className="shrink-0 text-muted-foreground" />;
}

export function smartMailboxFallbackIcon(name: string): LucideIcon {
  return name.toLowerCase() === "all mail" ? Mail : Folder;
}
