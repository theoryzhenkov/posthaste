/**
 * Root application component: QueryClientProvider, toolbar, three-column layout,
 * and settings panel.
 *
 * @spec docs/L1-ui#component-hierarchy
 * @spec docs/L0-ui#navigation-model
 */
import { QueryClient, QueryClientProvider, useQuery } from "@tanstack/react-query";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Loader2 } from "lucide-react";
import { useDefaultLayout } from "react-resizable-panels";
import { Toaster } from "sonner";
import { fetchAccounts, fetchMessage } from "./api/client";
import type { MessageSummary } from "./api/types";
import { ActionBar } from "./components/ActionBar";
import { MessageDetail } from "./components/MessageDetail";
import { MessageList } from "./components/MessageList";
import { SettingsPanel } from "./components/SettingsPanel";
import { ShortcutReference } from "./components/ShortcutReference";
import { Sidebar, type SidebarSelection } from "./components/Sidebar";
import { DesignThemeProvider } from "./components/ThemeProvider";
import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from "./components/ui/resizable";
import { useDaemonEvents } from "./hooks/useDaemonEvents";
import { useDebouncedValue } from "./hooks/useDebouncedValue";
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
  const debouncedQuery = useDebouncedValue(searchQuery, 300);
  const searchInputRef = useRef<HTMLInputElement>(null);
  const [showShortcuts, setShowShortcuts] = useState(false);

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      const target = event.target as HTMLElement;
      if (target.tagName === "INPUT" || target.tagName === "TEXTAREA") return;
      if (event.key === "?") {
        setShowShortcuts((prev) => !prev);
      }
      if (event.key === "/") {
        event.preventDefault();
        searchInputRef.current?.focus();
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

  const handleSearch = useCallback((query: string, append?: boolean) => {
    setSearchQuery((prev) => (append && prev ? `${prev} ${query}` : query));
  }, []);

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
    setSearchQuery("");
  }

  function handleSelectSourceMailbox(sourceId: string, mailboxId: string, name: string) {
    setSelectedView({ kind: "source-mailbox", sourceId, mailboxId, name });
    setSelectedMessage(null);
    setSearchQuery("");
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
      <ActionBar
        isFlagged={selectedMessageQuery.data?.isFlagged ?? false}
        isMessageSelected={selectedMessage !== null}
        isSettingsOpen={isSettingsOpen}
        searchInputRef={searchInputRef}
        searchQuery={searchQuery}
        onArchive={() =>
          selectedMessage &&
          actions.archive({
            sourceId: selectedMessage.sourceId,
            messageId: selectedMessage.messageId,
          })
        }
        onClearSearch={() => setSearchQuery("")}
        onSearchQueryChange={setSearchQuery}
        onShowShortcuts={() => setShowShortcuts(true)}
        onToggleFlag={() =>
          selectedMessage &&
          actions.toggleFlag({
            conversationId: selectedMessage.conversationId,
            sourceId: selectedMessage.sourceId,
            messageId: selectedMessage.messageId,
            isFlagged: selectedMessageQuery.data?.isFlagged ?? false,
            isRead: selectedMessageQuery.data?.isRead,
            keywords: selectedMessageQuery.data?.keywords,
          })
        }
        onToggleSettings={() => setIsSettingsPinned((open) => !open)}
        onTrash={() =>
          selectedMessage &&
          actions.trash({
            sourceId: selectedMessage.sourceId,
            messageId: selectedMessage.messageId,
          })
        }
      />
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
              searchQuery={debouncedQuery}
            />
          </ResizablePanel>
          <ResizableHandle />
          <ResizablePanel id="message-detail" minSize="300px">
            <MessageDetail
              selection={selectedMessage}
              onSelectMessage={handleSelectMessage}
              onSearch={handleSearch}
            />
          </ResizablePanel>
        </ResizablePanelGroup>
      )}

      {showShortcuts && <ShortcutReference onClose={() => setShowShortcuts(false)} />}
    </div>
  );
}

/**
 * Root App component: wraps `MailClient` in a `QueryClientProvider`.
 * @spec docs/L1-ui#component-hierarchy
 */
export default function App() {
  return (
    <DesignThemeProvider>
      <QueryClientProvider client={queryClient}>
        <MailClient />
        <Toaster
          position="bottom-center"
          toastOptions={{
            className: "font-sans text-sm",
          }}
        />
      </QueryClientProvider>
    </DesignThemeProvider>
  );
}
