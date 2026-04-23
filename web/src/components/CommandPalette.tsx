import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Archive,
  Clock3,
  Keyboard,
  MessageSquareText,
  PenSquare,
  Reply,
  Settings,
  SlidersHorizontal,
  Tag,
  User,
  UserPlus,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";

import { fetchSidebar } from "@/api/client";
import type { ConversationSummary } from "@/api/types";
import { renderMailboxRoleIcon, smartMailboxFallbackIcon } from "@/mailboxRoles";

import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList, CommandShortcut } from "./ui/command";

type SettingsCategory = "general" | "accounts" | "mailboxes";

type PaletteCommandId =
  | "compose"
  | "reply"
  | "archive"
  | "flag"
  | "snooze"
  | "newSmart"
  | "newRule"
  | "settings"
  | "shortcuts"
  | "account";

type PaletteEntry =
  | {
      id: string;
      kind: "command";
      label: string;
      keywords: string;
      shortcut?: string;
      icon: React.ReactNode;
      onSelect: () => void;
    }
  | {
      id: string;
      kind: "message";
      label: string;
      sub: string;
      keywords: string;
      icon: React.ReactNode;
      onSelect: () => void;
    }
  | {
      id: string;
      kind: "contact";
      label: string;
      keywords: string;
      icon: React.ReactNode;
      onSelect: () => void;
    }
  | {
      id: string;
      kind: "mailbox";
      label: string;
      sub: string;
      keywords: string;
      icon: React.ReactNode;
      onSelect: () => void;
    };

interface CommandPaletteProps {
  hasSelectedMessage: boolean;
  onApplySearch: (query: string) => void;
  onArchive: () => void;
  onClose: () => void;
  onCompose: () => void;
  onOpenSettings: (category?: SettingsCategory) => void;
  onOpenShortcuts: () => void;
  onPlaceholderAction: (label: string) => void;
  onReply: () => void;
  onSelectConversation: (conversation: ConversationSummary) => void;
  onSelectSmartMailbox: (smartMailboxId: string, name: string) => void;
  onSelectSourceMailbox: (sourceId: string, mailboxId: string, name: string) => void;
  onToggleFlag: () => void;
}

function normalizeQuery(value: string): string {
  return value.trim().toLowerCase();
}

function matchesQuery(query: string, text: string): boolean {
  return query.length === 0 || text.toLowerCase().includes(query);
}

function formatMessageSubline(conversation: ConversationSummary): string {
  const sender = conversation.fromName ?? conversation.fromEmail ?? "Unknown";
  const received = new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
  }).format(new Date(conversation.latestReceivedAt));
  return `${sender} · ${received}`;
}

function commandIcon(id: PaletteCommandId): React.ReactNode {
  switch (id) {
    case "compose":
      return <PenSquare size={15} strokeWidth={1.7} className="text-muted-foreground" />;
    case "reply":
      return <Reply size={15} strokeWidth={1.7} className="text-muted-foreground" />;
    case "archive":
      return <Archive size={15} strokeWidth={1.7} className="text-muted-foreground" />;
    case "flag":
      return <Tag size={15} strokeWidth={1.7} className="text-muted-foreground" />;
    case "snooze":
      return <Clock3 size={15} strokeWidth={1.7} className="text-muted-foreground" />;
    case "newSmart":
    case "newRule":
      return <SlidersHorizontal size={15} strokeWidth={1.7} className="text-muted-foreground" />;
    case "settings":
      return <Settings size={15} strokeWidth={1.7} className="text-muted-foreground" />;
    case "shortcuts":
      return <Keyboard size={15} strokeWidth={1.7} className="text-muted-foreground" />;
    case "account":
      return <UserPlus size={15} strokeWidth={1.7} className="text-muted-foreground" />;
  }
}

export function CommandPalette({
  hasSelectedMessage,
  onApplySearch,
  onArchive,
  onClose,
  onCompose,
  onOpenSettings,
  onOpenShortcuts,
  onPlaceholderAction,
  onReply,
  onSelectConversation,
  onSelectSmartMailbox,
  onSelectSourceMailbox,
  onToggleFlag,
}: CommandPaletteProps) {
  const [query, setQuery] = useState("");
  const panelRef = useRef<HTMLDivElement>(null);
  const queryClient = useQueryClient();
  const { data: sidebar } = useQuery({
    queryKey: ["sidebar"],
    queryFn: fetchSidebar,
  });

  const cachedConversations = useMemo(() => {
    const deduped = new Map<string, ConversationSummary>();
    for (const [, conversation] of queryClient.getQueriesData<ConversationSummary>({
      queryKey: ["conversation-summary"],
    })) {
      if (conversation) {
        deduped.set(conversation.id, conversation);
      }
    }
    return [...deduped.values()].sort((left, right) =>
      right.latestReceivedAt.localeCompare(left.latestReceivedAt),
    );
  }, [queryClient]);

  const results = useMemo(() => {
    const normalized = normalizeQuery(query);

    const commands: PaletteEntry[] = [
      {
        id: "compose",
        kind: "command" as const,
        label: "Compose new message",
        shortcut: "⌘N",
        keywords: "compose new message draft",
        icon: commandIcon("compose"),
        onSelect: onCompose,
      },
      {
        id: "reply",
        kind: "command" as const,
        label: "Reply",
        shortcut: "⌘R",
        keywords: "reply respond answer",
        icon: commandIcon("reply"),
        onSelect: onReply,
      },
      {
        id: "archive",
        kind: "command" as const,
        label: "Archive selected",
        shortcut: "E",
        keywords: "archive selected",
        icon: commandIcon("archive"),
        onSelect: onArchive,
      },
      {
        id: "flag",
        kind: "command" as const,
        label: "Flag message",
        shortcut: "⇧⌘L",
        keywords: "flag star selected",
        icon: commandIcon("flag"),
        onSelect: onToggleFlag,
      },
      {
        id: "snooze",
        kind: "command" as const,
        label: "Snooze…",
        shortcut: "H",
        keywords: "snooze later remind",
        icon: commandIcon("snooze"),
        onSelect: () => onPlaceholderAction("Snooze"),
      },
      {
        id: "newSmart",
        kind: "command" as const,
        label: "New smart mailbox…",
        keywords: "new smart mailbox create filter",
        icon: commandIcon("newSmart"),
        onSelect: () => onOpenSettings("mailboxes"),
      },
      {
        id: "newRule",
        kind: "command" as const,
        label: "New rule for mailbox…",
        keywords: "rule mailbox saved search",
        icon: commandIcon("newRule"),
        onSelect: () => onOpenSettings("mailboxes"),
      },
      {
        id: "settings",
        kind: "command" as const,
        label: "Open Settings",
        shortcut: "⌘,",
        keywords: "settings preferences",
        icon: commandIcon("settings"),
        onSelect: () => onOpenSettings(),
      },
      {
        id: "shortcuts",
        kind: "command" as const,
        label: "Keyboard shortcuts",
        shortcut: "?",
        keywords: "keyboard shortcuts help",
        icon: commandIcon("shortcuts"),
        onSelect: onOpenShortcuts,
      },
      {
        id: "account",
        kind: "command" as const,
        label: "Add account…",
        keywords: "account add source login",
        icon: commandIcon("account"),
        onSelect: () => onOpenSettings("accounts"),
      },
    ].filter(
      (entry) =>
        matchesQuery(normalized, `${entry.label} ${entry.keywords}`) &&
        (hasSelectedMessage || !["archive", "flag", "reply"].includes(entry.id)),
    );

    const messages = cachedConversations
      .filter((conversation) =>
        matchesQuery(
          normalized,
          [
            conversation.subject,
            conversation.preview,
            conversation.fromName,
            conversation.fromEmail,
          ]
            .filter(Boolean)
            .join(" "),
        ),
      )
      .slice(0, 6)
      .map<PaletteEntry>((conversation) => ({
        id: conversation.id,
        kind: "message",
        label: conversation.subject ?? "(no subject)",
        sub: formatMessageSubline(conversation),
        keywords: `${conversation.subject ?? ""} ${conversation.preview ?? ""} ${conversation.fromName ?? ""} ${conversation.fromEmail ?? ""}`,
        icon: <MessageSquareText size={15} strokeWidth={1.7} className="text-muted-foreground" />,
        onSelect: () => onSelectConversation(conversation),
      }));

    const contacts = [...new Set(cachedConversations.map((conversation) => conversation.fromName ?? conversation.fromEmail).filter(Boolean))]
      .filter((contact): contact is string => Boolean(contact))
      .filter((contact) => matchesQuery(normalized, contact))
      .slice(0, 5)
      .map<PaletteEntry>((contact) => ({
        id: `contact:${contact}`,
        kind: "contact",
        label: contact,
        keywords: contact,
        icon: <User size={15} strokeWidth={1.7} className="text-muted-foreground" />,
        onSelect: () => onApplySearch(contact),
      }));

    const mailboxes: PaletteEntry[] = [];
    if (sidebar) {
      for (const smartMailbox of sidebar.smartMailboxes) {
        if (matchesQuery(normalized, smartMailbox.name)) {
          mailboxes.push({
            id: `smart:${smartMailbox.id}`,
            kind: "mailbox",
            label: smartMailbox.name,
            sub: "Smart mailbox",
            keywords: smartMailbox.name,
            icon: renderMailboxRoleIcon(null, 15, smartMailboxFallbackIcon(smartMailbox.name)),
            onSelect: () => onSelectSmartMailbox(smartMailbox.id, smartMailbox.name),
          });
        }
      }
      for (const source of sidebar.sources) {
        for (const mailbox of source.mailboxes) {
          const haystack = `${mailbox.name} ${source.name}`;
          if (matchesQuery(normalized, haystack)) {
            mailboxes.push({
              id: `${source.id}:${mailbox.id}`,
              kind: "mailbox",
              label: mailbox.name,
              sub: source.name,
              keywords: haystack,
              icon: renderMailboxRoleIcon(mailbox.role, 15),
              onSelect: () =>
                onSelectSourceMailbox(source.id, mailbox.id, `${source.name} / ${mailbox.name}`),
            });
          }
        }
      }
    }

    return [
      { label: "Commands", items: commands },
      { label: "Messages", items: messages },
      { label: "Contacts", items: contacts },
      { label: "Mailboxes", items: mailboxes.slice(0, 6) },
    ].filter((group) => group.items.length > 0);
  }, [
    cachedConversations,
    hasSelectedMessage,
    onApplySearch,
    onArchive,
    onCompose,
    onOpenSettings,
    onOpenShortcuts,
    onPlaceholderAction,
    onReply,
    onSelectConversation,
    onSelectSmartMailbox,
    onSelectSourceMailbox,
    onToggleFlag,
    query,
    sidebar,
  ]);

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        event.preventDefault();
        onClose();
      }
    }

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [onClose]);

  function handleBackdropClick(event: React.MouseEvent<HTMLDivElement>) {
    if (panelRef.current && !panelRef.current.contains(event.target as Node)) {
      onClose();
    }
  }

  return (
    <div
      className="fixed inset-0 z-[70] flex items-start justify-center bg-[rgba(6,4,12,0.46)] px-4 pt-[9vh] backdrop-blur-[22px] backdrop-saturate-150"
      onMouseDown={handleBackdropClick}
    >
      <div
        ref={panelRef}
        className="w-full max-w-[40rem] overflow-hidden rounded-[14px] border border-white/10 bg-[rgba(22,20,28,0.88)] text-white shadow-[0_28px_80px_rgba(0,0,0,0.6)]"
      >
        <Command shouldFilter={false} loop>
          <div className="border-b border-white/10 px-4">
            <div className="flex items-center gap-3">
              <CommandInput
                autoFocus
                value={query}
                onValueChange={setQuery}
                placeholder="Search messages, contacts, commands…"
                className="text-white placeholder:text-white/42"
                wrapperClassName="h-12"
              />
              <CommandShortcut className="border-white/12 bg-white/8 text-white/58">
                Esc
              </CommandShortcut>
            </div>
          </div>

          <CommandList className="ph-scroll px-0 py-1.5">
            <CommandEmpty>No results. Try a different query.</CommandEmpty>
            {results.map((group) => (
              <CommandGroup key={group.label} heading={group.label} className="py-1">
                {group.items.map((item) => (
                  <CommandItem
                    key={item.id}
                    value={`${item.kind}:${item.id}:${item.keywords}`}
                    className="mx-0 px-4 py-2.5 text-white/94 data-[selected=true]:bg-white/8"
                    onSelect={() => {
                      item.onSelect();
                      onClose();
                    }}
                  >
                    <span className="flex size-4 shrink-0 items-center justify-center">
                      {item.icon}
                    </span>
                    <span className="min-w-0 flex-1 truncate">{item.label}</span>
                    {"sub" in item && item.sub && (
                      <span className="max-w-[14rem] truncate text-[12px] text-white/52">
                        {item.sub}
                      </span>
                    )}
                    {"shortcut" in item && item.shortcut && (
                      <CommandShortcut className="border-white/12 bg-white/8 text-white/58">
                        {item.shortcut}
                      </CommandShortcut>
                    )}
                  </CommandItem>
                ))}
              </CommandGroup>
            ))}
          </CommandList>

          <div className="flex items-center gap-4 border-t border-white/10 px-4 py-2 font-mono text-[10px] font-semibold tracking-[0.08em] text-white/48">
            <span className="flex items-center gap-1.5">
              <CommandShortcut className="border-white/12 bg-white/8 text-white/58">↑</CommandShortcut>
              <CommandShortcut className="border-white/12 bg-white/8 text-white/58">↓</CommandShortcut>
              navigate
            </span>
            <span className="flex items-center gap-1.5">
              <CommandShortcut className="border-white/12 bg-white/8 text-white/58">↵</CommandShortcut>
              select
            </span>
            <span className="flex items-center gap-1.5">
              <CommandShortcut className="border-white/12 bg-white/8 text-white/58">Esc</CommandShortcut>
              close
            </span>
            <div className="flex-1" />
            <span className="uppercase tracking-[0.18em] text-white/40">posthaste</span>
          </div>
        </Command>
      </div>
    </div>
  );
}
