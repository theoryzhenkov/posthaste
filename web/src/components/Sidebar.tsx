/**
 * Left-pane sidebar with smart mailbox and source mailbox navigation.
 *
 * Loads data from `GET /v1/sidebar` and renders collapsible sections
 * for smart mailboxes and per-source mailbox trees.
 *
 * @spec spec/L1-ui#component-hierarchy
 * @spec spec/L0-ui#navigation-model
 */
import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import { fetchSidebar } from "../api/client";
import type { Mailbox, SidebarResponse } from "../api/types";
import { cn } from "../lib/utils";
import {
  mailboxRoleFromName,
  renderMailboxRoleIcon,
  smartMailboxFallbackIcon,
} from "../mailboxRoles";

/**
 * Discriminated union representing the current sidebar selection.
 * @spec spec/L0-ui#navigation-model
 */
export type SidebarSelection =
  | { kind: "smart-mailbox"; id: string; name: string }
  | { kind: "source-mailbox"; sourceId: string; mailboxId: string; name: string };

/** @spec spec/L1-ui#component-hierarchy */
interface SidebarProps {
  selectedView: SidebarSelection | null;
  onSelectSmartMailbox: (smartMailboxId: string, name: string) => void;
  onSelectSourceMailbox: (sourceId: string, mailboxId: string, name: string) => void;
}

function roleIcon(role: Mailbox["role"], size = 14): React.ReactNode {
  return renderMailboxRoleIcon(role, size);
}

/** Icon for smart mailboxes based on the name heuristic. */
function smartMailboxIcon(name: string, size = 14): React.ReactNode {
  return renderMailboxRoleIcon(
    mailboxRoleFromName(name),
    size,
    smartMailboxFallbackIcon(name),
  );
}

function itemButtonClass(isSelected: boolean): string {
  return cn(
    "flex w-full items-center gap-2 px-3 py-1.5 text-left text-sm transition-colors",
    "hover:bg-accent",
    isSelected && "border-l-2 border-l-primary bg-accent text-accent-foreground",
    !isSelected && "border-l-2 border-l-transparent",
  );
}

/** Smart mailbox row with unread badge. */
function ViewItem({
  name,
  unreadMessages,
  isSelected,
  onSelect,
}: {
  name: string;
  unreadMessages?: number;
  isSelected: boolean;
  onSelect: () => void;
}) {
  return (
    <button className={itemButtonClass(isSelected)} onClick={onSelect} type="button">
      {smartMailboxIcon(name)}
      <span className="min-w-0 flex-1 truncate">{name}</span>
      {unreadMessages != null && unreadMessages > 0 && (
        <span className="font-mono text-xs tabular-nums text-primary">
          {unreadMessages}
        </span>
      )}
    </button>
  );
}

/** Source mailbox row with role icon and unread badge. */
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
    <button className={itemButtonClass(isSelected)} onClick={onSelect} type="button">
      {roleIcon(mailbox.role)}
      <span className="min-w-0 flex-1 truncate">{mailbox.name}</span>
      {mailbox.unreadEmails > 0 && (
        <span className="font-mono text-xs tabular-nums text-primary">
          {mailbox.unreadEmails}
        </span>
      )}
    </button>
  );
}

/** Collapsible source section with its mailbox children. */
function SourceSection({
  source,
  selectedView,
  onSelectSourceMailbox,
}: {
  source: SidebarResponse["sources"][number];
  selectedView: SidebarSelection | null;
  onSelectSourceMailbox: (sourceId: string, mailboxId: string, name: string) => void;
}) {
  const [collapsed, setCollapsed] = useState(false);

  return (
    <div>
      <button
        type="button"
        className="flex w-full items-center gap-1.5 px-3 py-1 text-[10px] font-mono uppercase tracking-wider text-muted-foreground hover:text-foreground transition-colors"
        onClick={() => setCollapsed((prev) => !prev)}
      >
        {collapsed ? <ChevronRight size={12} /> : <ChevronDown size={12} />}
        <span>{source.name}</span>
      </button>
      {!collapsed && (
        <div className="space-y-0.5">
          {source.mailboxes.map((mailbox) => (
            <div key={`${source.id}:${mailbox.id}`} className="pl-2">
              <MailboxItem
                mailbox={mailbox}
                isSelected={
                  selectedView?.kind === "source-mailbox" &&
                  selectedView.sourceId === source.id &&
                  selectedView.mailboxId === mailbox.id
                }
                onSelect={() =>
                  onSelectSourceMailbox(source.id, mailbox.id, `${source.name} / ${mailbox.name}`)
                }
              />
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

/** Collapsible section header button. */
function SectionHeader({
  label,
  collapsed,
  onToggle,
}: {
  label: string;
  collapsed: boolean;
  onToggle: () => void;
}) {
  return (
    <button
      type="button"
      className="flex w-full items-center gap-1.5 border-b border-border px-3 py-1.5 text-[10px] font-mono uppercase tracking-wider text-muted-foreground hover:text-foreground transition-colors"
      onClick={onToggle}
    >
      {collapsed ? <ChevronRight size={12} /> : <ChevronDown size={12} />}
      <span>{label}</span>
    </button>
  );
}

/**
 * Sidebar navigation with smart mailbox and source mailbox sections.
 *
 * @spec spec/L1-ui#component-hierarchy
 * @spec spec/L0-ui#navigation-model
 */
export function Sidebar({
  selectedView,
  onSelectSmartMailbox,
  onSelectSourceMailbox,
}: SidebarProps) {
  const { data: sidebar, isLoading, error } = useQuery({
    queryKey: ["sidebar"],
    queryFn: fetchSidebar,
  });

  const [mailboxesCollapsed, setMailboxesCollapsed] = useState(false);
  const [sourcesCollapsed, setSourcesCollapsed] = useState(false);

  return (
    <aside className="flex h-full min-h-0 min-w-0 flex-col border-r border-border bg-sidebar text-sidebar-foreground">
      <nav className="flex-1 min-h-0 overflow-y-auto pt-1">
        {isLoading && <p className="px-3 py-2 text-xs text-muted-foreground">Loading...</p>}
        {error && (
          <p className="px-3 py-2 text-xs text-destructive">Failed to load sidebar</p>
        )}
        {sidebar && (
          <>
            <SectionHeader
              label="Mailboxes"
              collapsed={mailboxesCollapsed}
              onToggle={() => setMailboxesCollapsed((prev) => !prev)}
            />
            {!mailboxesCollapsed && (
              <div className="py-1">
                {sidebar.smartMailboxes.map((smartMailbox) => (
                  <ViewItem
                    key={smartMailbox.id}
                    name={smartMailbox.name}
                    unreadMessages={smartMailbox.unreadMessages}
                    isSelected={
                      selectedView?.kind === "smart-mailbox" &&
                      selectedView.id === smartMailbox.id
                    }
                    onSelect={() => onSelectSmartMailbox(smartMailbox.id, smartMailbox.name)}
                  />
                ))}
              </div>
            )}

            <SectionHeader
              label="Sources"
              collapsed={sourcesCollapsed}
              onToggle={() => setSourcesCollapsed((prev) => !prev)}
            />
            {!sourcesCollapsed && (
              <div className="space-y-2 py-1">
                {sidebar.sources.map((source) => (
                  <SourceSection
                    key={source.id}
                    source={source}
                    selectedView={selectedView}
                    onSelectSourceMailbox={onSelectSourceMailbox}
                  />
                ))}
              </div>
            )}
          </>
        )}
      </nav>
    </aside>
  );
}
