import { useQuery } from "@tanstack/react-query";
import { fetchMailboxes } from "../api/client";
import type { Mailbox } from "../api/types";

interface SidebarProps {
  accountId: string;
  selectedMailboxId: string | null;
  onSelectMailbox: (id: string) => void;
}

const ROLE_ICONS: Record<string, string> = {
  inbox: "📥",
  sent: "📤",
  drafts: "📝",
  trash: "🗑️",
  junk: "⚠️",
  archive: "📦",
};

function mailboxIcon(role: string | null): string {
  if (role && role in ROLE_ICONS) {
    return ROLE_ICONS[role];
  }
  return "📁";
}

function MailboxItem({
  mailbox,
  isSelected,
  onSelect,
}: {
  mailbox: Mailbox;
  isSelected: boolean;
  onSelect: () => void;
}) {
  return (
    <button
      className={`sidebar-item ${isSelected ? "sidebar-item--selected" : ""}`}
      onClick={onSelect}
      type="button"
    >
      <span className="sidebar-item__icon">{mailboxIcon(mailbox.role)}</span>
      <span className="sidebar-item__name">{mailbox.name}</span>
      {mailbox.unreadEmails > 0 && (
        <span className="sidebar-item__badge">{mailbox.unreadEmails}</span>
      )}
    </button>
  );
}

export function Sidebar({
  accountId,
  selectedMailboxId,
  onSelectMailbox,
}: SidebarProps) {
  const { data: mailboxes, isLoading, error } = useQuery({
    queryKey: ["mailboxes", accountId],
    queryFn: () => fetchMailboxes(accountId),
  });

  return (
    <aside className="sidebar">
      <div className="sidebar__header">
        <div className="sidebar__brand">
          <div className="sidebar__brand-mark">m</div>
          <div>
            <p className="sidebar__eyebrow">daemon-backed</p>
            <h2 className="sidebar__title">Folders</h2>
          </div>
        </div>
      </div>
      <p className="sidebar__section-label">mailboxes</p>
      <nav className="sidebar__nav">
        {isLoading && <p className="sidebar__status">Loading...</p>}
        {error && (
          <p className="sidebar__status sidebar__status--error">
            Failed to load mailboxes
          </p>
        )}
        {mailboxes?.map((mailbox) => (
          <MailboxItem
            key={mailbox.id}
            mailbox={mailbox}
            isSelected={mailbox.id === selectedMailboxId}
            onSelect={() => onSelectMailbox(mailbox.id)}
          />
        ))}
      </nav>
      <div className="sidebar__footer">
        <span>local API</span>
        <span>SSE sync</span>
      </div>
    </aside>
  );
}
