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
import { toast, Toaster } from "sonner";
import { fetchAccounts, fetchMessage } from "./api/client";
import type { ConversationSummary, MessageSummary } from "./api/types";
import { ActionBar } from "./components/ActionBar";
import { CommandPalette } from "./components/CommandPalette";
import { ComposeOverlay, type ComposeIntent } from "./components/ComposeOverlay";
import { MessageDetail } from "./components/MessageDetail";
import { MessageList } from "./components/MessageList";
import { SettingsOverlay } from "./components/SettingsOverlay";
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
import { useDesignTheme } from "./hooks/useDesignTheme";
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
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);
  const [settingsCategory, setSettingsCategory] =
    useState<"general" | "accounts" | "mailboxes" | null>(null);
  const [isCommandPaletteOpen, setIsCommandPaletteOpen] = useState(false);
  const [composeIntent, setComposeIntent] = useState<ComposeIntent | null>(null);
  const [isSearchActive, setIsSearchActive] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const debouncedQuery = useDebouncedValue(searchQuery, 300);
  const searchInputRef = useRef<HTMLInputElement>(null);
  const [showShortcuts, setShowShortcuts] = useState(false);
  const theme = useDesignTheme();

  const handlePlaceholderAction = useCallback((label: string) => {
    toast(`${label} is not available yet.`);
  }, []);

  const handleToggleTheme = useCallback(() => {
    theme.setMode(theme.resolvedMode === "dark" ? "light" : "dark");
  }, [theme]);

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
  const shouldForceSettings = accounts.length === 0;
  const showSettings = isSettingsOpen || shouldForceSettings;
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

  const handleToggleFlag = useCallback(() => {
    if (!selectedMessage) {
      return;
    }
    actions.toggleFlag({
      conversationId: selectedMessage.conversationId,
      sourceId: selectedMessage.sourceId,
      messageId: selectedMessage.messageId,
      isFlagged: selectedMessageQuery.data?.isFlagged ?? false,
      isRead: selectedMessageQuery.data?.isRead,
      keywords: selectedMessageQuery.data?.keywords,
    });
  }, [actions, selectedMessage, selectedMessageQuery.data]);

  const handleArchive = useCallback(() => {
    if (!selectedMessage) {
      return;
    }
    actions.archive({
      sourceId: selectedMessage.sourceId,
      messageId: selectedMessage.messageId,
    });
  }, [actions, selectedMessage]);

  const handleTrash = useCallback(() => {
    if (!selectedMessage) {
      return;
    }
    actions.trash({
      sourceId: selectedMessage.sourceId,
      messageId: selectedMessage.messageId,
    });
  }, [actions, selectedMessage]);

  const resolveComposeSourceId = useCallback(() => {
    return (
      selectedMessage?.sourceId ??
      (effectiveView?.kind === "source-mailbox" ? effectiveView.sourceId : null) ??
      enabledAccounts[0]?.id ??
      null
    );
  }, [effectiveView, enabledAccounts, selectedMessage]);

  const handleCompose = useCallback(() => {
    const sourceId = resolveComposeSourceId();
    if (!sourceId) {
      setSettingsCategory("accounts");
      setIsSettingsOpen(true);
      return;
    }
    setComposeIntent({ kind: "new", sourceId });
  }, [resolveComposeSourceId]);

  const handleReply = useCallback(() => {
    if (!selectedMessage) {
      return;
    }
    setComposeIntent({
      kind: "reply",
      sourceId: selectedMessage.sourceId,
      messageId: selectedMessage.messageId,
    });
  }, [selectedMessage]);

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      const target = event.target as HTMLElement;
      const isTypingTarget =
        target.tagName === "INPUT" ||
        target.tagName === "TEXTAREA" ||
        target.isContentEditable;

      if ((event.metaKey || event.ctrlKey) && (event.key === "k" || event.key === "K")) {
        event.preventDefault();
        setIsCommandPaletteOpen(true);
        return;
      }
      if ((event.metaKey || event.ctrlKey) && event.key === ",") {
        event.preventDefault();
        setSettingsCategory(null);
        setIsSettingsOpen(true);
        return;
      }
      if ((event.metaKey || event.ctrlKey) && (event.key === "n" || event.key === "N")) {
        event.preventDefault();
        handleCompose();
        return;
      }
      if ((event.metaKey || event.ctrlKey) && (event.key === "r" || event.key === "R")) {
        event.preventDefault();
        handleReply();
        return;
      }
      if ((event.metaKey || event.ctrlKey) && event.shiftKey && event.key.toLowerCase() === "l") {
        event.preventDefault();
        if (selectedMessage) {
          handleToggleFlag();
        }
        return;
      }
      if (isTypingTarget) return;
      if (event.key === "?") {
        event.preventDefault();
        setShowShortcuts((prev) => !prev);
        return;
      }
      if (event.key === "/") {
        event.preventDefault();
        setIsSearchActive(true);
        requestAnimationFrame(() => searchInputRef.current?.focus());
      }
    }

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [handleCompose, handleReply, handleToggleFlag, selectedMessage]);

  const handleSearch = useCallback((query: string, append?: boolean) => {
    setSearchQuery((prev) => (append && prev ? `${prev} ${query}` : query));
    setIsSearchActive(true);
  }, []);

  const handleOpenSettings = useCallback(
    (category?: "general" | "accounts" | "mailboxes") => {
      setSettingsCategory(category ?? null);
      setIsSettingsOpen(true);
      setIsCommandPaletteOpen(false);
    },
    [],
  );

  const handleApplySearch = useCallback((query: string) => {
    setSearchQuery(query);
    setIsSearchActive(true);
    requestAnimationFrame(() => searchInputRef.current?.focus());
  }, []);

  const handleSelectConversation = useCallback((conversation: ConversationSummary) => {
    setSelectedMessage({
      conversationId: conversation.id,
      sourceId: conversation.latestMessage.sourceId,
      messageId: conversation.latestMessage.messageId,
    });
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
    setIsSearchActive(false);
  }

  function handleSelectSourceMailbox(sourceId: string, mailboxId: string, name: string) {
    setSelectedView({ kind: "source-mailbox", sourceId, mailboxId, name });
    setSelectedMessage(null);
    setSearchQuery("");
    setIsSearchActive(false);
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
        isDarkMode={theme.resolvedMode === "dark"}
        isFlagged={selectedMessageQuery.data?.isFlagged ?? false}
        isMessageSelected={selectedMessage !== null}
        isSearchActive={isSearchActive}
        isSettingsOpen={showSettings}
        searchInputRef={searchInputRef}
        searchQuery={searchQuery}
        onArchive={handleArchive}
        onClearSearch={() => {
          setSearchQuery("");
          setIsSearchActive(false);
        }}
        onCompose={handleCompose}
        onFocusSearch={() => setIsSearchActive(true)}
        onOpenCommandPalette={() => setIsCommandPaletteOpen(true)}
        onPlaceholderAction={handlePlaceholderAction}
        onReply={handleReply}
        onSearchBlur={() => setIsSearchActive(false)}
        onSearchQueryChange={setSearchQuery}
        onShowShortcuts={() => setShowShortcuts(true)}
        onToggleFlag={handleToggleFlag}
        onToggleSettings={() => {
          setSettingsCategory(null);
          setIsSettingsOpen((open) => !open);
        }}
        onToggleTheme={handleToggleTheme}
        onTrash={handleTrash}
      />
      {actions.errorMessage && (
        <div className="border-b border-destructive/20 bg-destructive/5 px-3 py-2 text-sm text-destructive">
          {actions.errorMessage}
        </div>
      )}

      {/* Main content */}
      <ResizablePanelGroup
        orientation="horizontal"
        defaultLayout={defaultLayout}
        onLayoutChanged={onLayoutChanged}
        className="min-h-0 flex-1"
      >
        <ResizablePanel
          id="sidebar"
          defaultSize="210px"
          minSize="190px"
          maxSize="420px"
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
          minSize="360px"
          maxSize="960px"
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

      {showSettings && (
        <SettingsOverlay
          accounts={accounts}
          activeAccountId={focusedSourceId}
          initialCategory={shouldForceSettings ? "accounts" : settingsCategory ?? undefined}
          onActiveAccountChange={() => {
            setSelectedView(DEFAULT_VIEW);
            setSelectedMessage(null);
          }}
          onClose={() => {
            if (!shouldForceSettings) {
              setIsSettingsOpen(false);
            }
          }}
        />
      )}

      {isCommandPaletteOpen && (
        <CommandPalette
          hasSelectedMessage={selectedMessage !== null}
          onApplySearch={handleApplySearch}
          onArchive={handleArchive}
          onClose={() => setIsCommandPaletteOpen(false)}
          onCompose={handleCompose}
          onOpenSettings={handleOpenSettings}
          onOpenShortcuts={() => setShowShortcuts(true)}
          onPlaceholderAction={handlePlaceholderAction}
          onReply={handleReply}
          onSelectConversation={handleSelectConversation}
          onSelectSmartMailbox={handleSelectSmartMailbox}
          onSelectSourceMailbox={handleSelectSourceMailbox}
          onToggleFlag={handleToggleFlag}
        />
      )}

      {showShortcuts && <ShortcutReference onClose={() => setShowShortcuts(false)} />}
      {composeIntent && (
        <ComposeOverlay
          intent={composeIntent}
          onClose={() => setComposeIntent(null)}
        />
      )}
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
