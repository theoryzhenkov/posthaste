/**
 * Root application component: QueryClientProvider, toolbar, three-column layout,
 * and settings panel.
 *
 * @spec docs/L1-ui#component-hierarchy
 * @spec docs/L0-ui#navigation-model
 */
import { QueryClient, QueryClientProvider, useQuery } from "@tanstack/react-query";
import { useEffect, useMemo, useState } from "react";
import { Archive, Loader2, Search, Settings, Star, Trash2, X } from "lucide-react";
import { Toaster } from "sonner";
import { fetchAccounts, fetchMessage } from "./api/client";
import type { MessageSummary } from "./api/types";
import { MessageDetail } from "./components/MessageDetail";
import { MessageList } from "./components/MessageList";
import { SettingsPanel } from "./components/SettingsPanel";
import { ShortcutReference } from "./components/ShortcutReference";
import { Sidebar, type SidebarSelection } from "./components/Sidebar";
import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
  useDefaultLayout,
} from "./components/ui/resizable";
import { Separator } from "./components/ui/separator";
import { cn } from "./lib/utils";
import { useDaemonEvents } from "./hooks/useDaemonEvents";
import { useEmailActions } from "./hooks/useEmailActions";
import { mailKeys, type MailSelection } from "./mailState";

/** @spec docs/L1-ui#data-fetching */
const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 30_000,
      retry: 1,
    },
  },
});

const DEFAULT_VIEW: SidebarSelection = {
  kind: "smart-mailbox",
  id: "default-inbox",
  name: "Inbox",
};

/**
 * Main mail client shell: toolbar, three-column layout, settings overlay.
 *
 * Manages view selection, message selection, SSE event subscription,
 * and keyboard-accessible email actions.
 *
 * @spec docs/L1-ui#component-hierarchy
 * @spec docs/L0-ui#navigation-model
 */
function MailClient() {
  const [selectedView, setSelectedView] = useState<SidebarSelection | null>(DEFAULT_VIEW);
  const [selectedMessage, setSelectedMessage] = useState<MailSelection | null>(null);
  const [isSettingsPinned, setIsSettingsPinned] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const [isSearchFocused, setIsSearchFocused] = useState(false);
  const [showShortcuts, setShowShortcuts] = useState(false);

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      const target = event.target as HTMLElement;
      if (target.tagName === "INPUT" || target.tagName === "TEXTAREA") return;
      if (event.key === "?") {
        setShowShortcuts((prev) => !prev);
      }
    }

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, []);

  const { data: accounts = [], isLoading } = useQuery({
    queryKey: ["accounts"],
    queryFn: fetchAccounts,
  });

  const enabledAccounts = useMemo(
    () => accounts.filter((account) => account.enabled),
    [accounts],
  );
  const hasEnabledSources = enabledAccounts.length > 0;
  const effectiveView = hasEnabledSources ? (selectedView ?? DEFAULT_VIEW) : null;
  const focusedSourceId =
    effectiveView?.kind === "source-mailbox" ? effectiveView.sourceId : null;
  const isSettingsOpen = isSettingsPinned || accounts.length === 0;
  const selectedMessageQuery = useQuery({
    queryKey: selectedMessage
      ? mailKeys.message(selectedMessage.sourceId, selectedMessage.messageId)
      : ["message", null, null],
    queryFn: () =>
      fetchMessage(selectedMessage!.messageId, selectedMessage!.sourceId),
    enabled: selectedMessage !== null,
  });

  useDaemonEvents();

  const { defaultLayout, onLayoutChanged } = useDefaultLayout({
    id: "posthaste-panels",
    storage: localStorage,
  });
  const actions = useEmailActions();

  function handleSelectMessage(message: MessageSummary) {
    setSelectedMessage({
      conversationId: message.conversationId,
      sourceId: message.sourceId,
      messageId: message.id,
    });
  }

  function handleSelectMessageRef(selection: MailSelection) {
    setSelectedMessage(selection);
  }

  function handleSelectSmartMailbox(smartMailboxId: string, name: string) {
    setSelectedView({ kind: "smart-mailbox", id: smartMailboxId, name });
    setSelectedMessage(null);
  }

  function handleSelectSourceMailbox(sourceId: string, mailboxId: string, name: string) {
    setSelectedView({ kind: "source-mailbox", sourceId, mailboxId, name });
    setSelectedMessage(null);
  }

  if (isLoading) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3">
        <Loader2 size={24} className="animate-spin text-muted-foreground" />
        <p className="text-sm text-muted-foreground">Setting up...</p>
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col overflow-hidden">
      {/* Toolbar */}
      <header className="flex items-center justify-between border-b border-border bg-card px-3 py-1.5">
        <span className="text-sm font-semibold tracking-tight select-none">PostHaste</span>

        <div className="flex items-center gap-1">
          {selectedMessage && (
            <>
              <ToolbarButton
                icon={<Archive size={16} />}
                title="Archive (e)"
                label="Archive"
                shortcut="e"
                onClick={() =>
                  actions.archive({
                    sourceId: selectedMessage.sourceId,
                    messageId: selectedMessage.messageId,
                  })
                }
              />
              <ToolbarButton
                icon={<Trash2 size={16} />}
                title="Trash (#)"
                label="Trash"
                shortcut="#"
                onClick={() =>
                  actions.trash({
                    sourceId: selectedMessage.sourceId,
                    messageId: selectedMessage.messageId,
                  })
                }
              />
              <ToolbarButton
                icon={
                  <Star
                    size={16}
                    className={
                      selectedMessageQuery.data?.isFlagged
                        ? "fill-amber-400 text-amber-400"
                        : undefined
                    }
                  />
                }
                title="Flag"
                onClick={() =>
                  actions.toggleFlag({
                    conversationId: selectedMessage.conversationId,
                    sourceId: selectedMessage.sourceId,
                    messageId: selectedMessage.messageId,
                    isFlagged: selectedMessageQuery.data?.isFlagged ?? false,
                    isRead: selectedMessageQuery.data?.isRead,
                    keywords: selectedMessageQuery.data?.keywords,
                  })
                }
              />
              <Separator orientation="vertical" className="mx-1.5 h-4" />
            </>
          )}

          <div className="relative flex items-center">
            <Search size={14} className="absolute left-2 text-muted-foreground" />
            <input
              type="text"
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              onFocus={() => setIsSearchFocused(true)}
              onBlur={() => setIsSearchFocused(false)}
              placeholder="Search..."
              className={cn(
                "h-7 rounded border border-border bg-background pl-7 pr-7 text-sm transition-all placeholder:text-muted-foreground/50 focus:outline-none focus:ring-1 focus:ring-ring",
                isSearchFocused ? "w-56" : "w-40",
              )}
            />
            {searchQuery && (
              <button
                type="button"
                className="absolute right-1.5 text-muted-foreground hover:text-foreground"
                onClick={() => setSearchQuery("")}
              >
                <X size={14} />
              </button>
            )}
          </div>
        </div>

        <button
          type="button"
          title="Settings"
          className={cn(
            "flex size-7 items-center justify-center rounded transition-colors hover:bg-accent",
            isSettingsOpen && "text-primary",
          )}
          onClick={() => setIsSettingsPinned((open) => !open)}
        >
          <Settings size={16} />
        </button>
      </header>
      {actions.errorMessage && (
        <div className="border-b border-destructive/20 bg-destructive/5 px-3 py-2 text-sm text-destructive">
          {actions.errorMessage}
        </div>
      )}

      {/* Main content */}
      {isSettingsOpen ? (
        <div className="min-h-0 flex-1">
          <SettingsPanel
            accounts={accounts}
            activeAccountId={focusedSourceId}
            onActiveAccountChange={() => {
              setSelectedView(DEFAULT_VIEW);
              setSelectedMessage(null);
            }}
          />
        </div>
      ) : (
        <ResizablePanelGroup
          orientation="horizontal"
          defaultLayout={defaultLayout}
          onLayoutChanged={onLayoutChanged}
          className="min-h-0 flex-1"
        >
          <ResizablePanel
            id="sidebar"
            defaultSize="220px"
            minSize="160px"
            maxSize="400px"
          >
            <Sidebar
              selectedView={effectiveView}
              onSelectSmartMailbox={handleSelectSmartMailbox}
              onSelectSourceMailbox={handleSelectSourceMailbox}
            />
          </ResizablePanel>
          <ResizableHandle />
          <ResizablePanel
            id="message-list"
            defaultSize="420px"
            minSize="280px"
            maxSize="800px"
          >
            <MessageList
              selectedView={effectiveView}
              selection={selectedMessage}
              onSelectMessage={handleSelectMessageRef}
              actions={actions}
            />
          </ResizablePanel>
          <ResizableHandle />
          <ResizablePanel id="message-detail" minSize="300px">
            <MessageDetail
              selection={selectedMessage}
              onSelectMessage={handleSelectMessage}
            />
          </ResizablePanel>
        </ResizablePanelGroup>
      )}

      {showShortcuts && <ShortcutReference onClose={() => setShowShortcuts(false)} />}
    </div>
  );
}

/** Toolbar button with optional text label and shortcut hint. */
function ToolbarButton({
  icon,
  title,
  label,
  shortcut,
  onClick,
}: {
  icon: React.ReactNode;
  title: string;
  label?: string;
  shortcut?: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      title={title}
      className={cn(
        "flex items-center justify-center rounded text-muted-foreground transition-colors hover:bg-accent hover:text-foreground",
        label ? "h-7 gap-1.5 px-2" : "size-7",
      )}
      onClick={onClick}
    >
      {icon}
      {label && <span className="text-xs">{label}</span>}
      {shortcut && (
        <kbd className="ml-1 text-[10px] text-muted-foreground/60">{shortcut}</kbd>
      )}
    </button>
  );
}

/**
 * Root App component: wraps `MailClient` in a `QueryClientProvider`.
 * @spec docs/L1-ui#component-hierarchy
 */
export default function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <MailClient />
      <Toaster
        position="bottom-center"
        toastOptions={{
          className: "font-sans text-sm",
        }}
      />
    </QueryClientProvider>
  );
}
