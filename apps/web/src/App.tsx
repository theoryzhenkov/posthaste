/**
 * Root application component: QueryClientProvider, toolbar, three-column layout,
 * and focused surfaces.
 *
 * @spec docs/L1-ui#component-hierarchy
 * @spec docs/L0-ui#navigation-model
 */
import {
  QueryClient,
  QueryClientProvider,
  useMutation,
  useQuery,
} from '@tanstack/react-query'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Loader2, X } from 'lucide-react'
import { useDefaultLayout } from 'react-resizable-panels'
import { toast, Toaster } from 'sonner'
import {
  fetchAccounts,
  fetchMessage,
  fetchSidebar,
  triggerSync,
} from './api/client'
import type { MessageSummary } from './api/types'
import { ActionBar } from './components/ActionBar'
import { CommandPalette } from './components/CommandPalette'
import { ComposeOverlay, type ComposeIntent } from './components/ComposeOverlay'
import { MessageDetail } from './components/MessageDetail'
import { MessageList } from './components/MessageList'
import { ShortcutReference } from './components/ShortcutReference'
import { Sidebar, type SidebarSelection } from './components/Sidebar'
import { SurfaceHost } from './components/SurfaceHost'
import { TagEditor } from './components/TagEditor'
import { DesignThemeProvider } from './components/ThemeProvider'
import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from './components/ui/resizable'
import { useDaemonEvents } from './hooks/useDaemonEvents'
import { useDesignTheme } from './hooks/useDesignTheme'
import { useEmailActions } from './hooks/useEmailActions'
import { mailKeys, type MailSelection } from './mailState'
import { queryKeys } from './queryKeys'
import {
  messageSurfaceFromSelection,
  settingsSurface,
  type SettingsSurfaceCategory,
  type SurfaceDescriptor,
} from './surfaces'
import {
  normalizeValidAppliedSearchQuery,
  prepareServerSearchQuery,
} from './searchQuery'

/** @spec docs/L1-ui#data-fetching */
const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 30_000,
      retry: 1,
    },
  },
})

const DEFAULT_VIEW: SidebarSelection = {
  kind: 'smart-mailbox',
  id: 'default-inbox',
  name: 'Inbox',
}
const SHELL_PANEL_IDS = ['sidebar', 'mail-content']

/**
 * Main mail client shell: toolbar, three-column layout, and surface host.
 *
 * Manages view selection, message selection, SSE event subscription,
 * and keyboard-accessible email actions.
 *
 * @spec docs/L1-ui#component-hierarchy
 * @spec docs/L0-ui#navigation-model
 */
function MailClient() {
  const [selectedView, setSelectedView] = useState<SidebarSelection | null>(
    DEFAULT_VIEW,
  )
  const [selectedMessage, setSelectedMessage] = useState<MailSelection | null>(
    null,
  )
  const [isCommandPaletteOpen, setIsCommandPaletteOpen] = useState(false)
  const [isTagEditorOpen, setIsTagEditorOpen] = useState(false)
  const [composeIntent, setComposeIntent] = useState<ComposeIntent | null>(null)
  const [searchQuery, setSearchQuery] = useState('')
  const lastAutoSeenKeyRef = useRef<string | null>(null)
  const [showShortcuts, setShowShortcuts] = useState(false)
  const [activeSurface, setActiveSurface] = useState<SurfaceDescriptor | null>(
    null,
  )
  const preparedSearchQuery = useMemo(
    () => prepareServerSearchQuery(searchQuery),
    [searchQuery],
  )
  const theme = useDesignTheme()

  const handlePlaceholderAction = useCallback((label: string) => {
    toast(`${label} is not available yet.`)
  }, [])

  const handleToggleTheme = useCallback(() => {
    theme.setMode(theme.resolvedMode === 'dark' ? 'light' : 'dark')
  }, [theme])

  const { data: accounts = [], isLoading } = useQuery({
    queryKey: queryKeys.accounts,
    queryFn: fetchAccounts,
  })
  const { data: sidebar } = useQuery({
    queryKey: queryKeys.sidebar,
    queryFn: fetchSidebar,
  })

  const enabledAccounts = useMemo(
    () => accounts.filter((account) => account.enabled),
    [accounts],
  )
  const hasEnabledSources = enabledAccounts.length > 0
  const effectiveView = hasEnabledSources
    ? (selectedView ?? DEFAULT_VIEW)
    : null
  const focusedSourceId =
    effectiveView?.kind === 'source-mailbox' ? effectiveView.sourceId : null
  const shouldForceSettings = accounts.length === 0
  const effectiveSurface =
    shouldForceSettings && activeSurface?.kind !== 'settings'
      ? settingsSurface({ category: 'accounts' })
      : activeSurface
  const isSettingsSurfaceOpen = effectiveSurface?.kind === 'settings'
  const selectedMessageQuery = useQuery({
    queryKey: selectedMessage
      ? mailKeys.message(selectedMessage.sourceId, selectedMessage.messageId)
      : ['message', null, null],
    queryFn: () =>
      fetchMessage(selectedMessage!.messageId, selectedMessage!.sourceId),
    enabled: selectedMessage !== null,
  })
  const isMessageDetailOpen = selectedMessage !== null
  const messagePanelIds = useMemo(
    () =>
      isMessageDetailOpen
        ? ['message-list', 'message-detail']
        : ['message-list'],
    [isMessageDetailOpen],
  )

  useDaemonEvents()

  const {
    defaultLayout: shellDefaultLayout,
    onLayoutChanged: onShellLayoutChanged,
  } = useDefaultLayout({
    id: 'posthaste-shell-panels',
    panelIds: SHELL_PANEL_IDS,
    storage: localStorage,
  })
  const {
    defaultLayout: messageDefaultLayout,
    onLayoutChanged: onMessageLayoutChanged,
  } = useDefaultLayout({
    id: 'posthaste-message-panels',
    panelIds: messagePanelIds,
    storage: localStorage,
  })
  const actions = useEmailActions()
  const syncSourceMutation = useMutation({
    mutationFn: triggerSync,
    onSuccess: async () => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: queryKeys.sidebar }),
        queryClient.invalidateQueries({ queryKey: queryKeys.messagesRoot }),
      ])
      toast('Sync started')
    },
    onError: (error) => {
      toast.error(error.message)
    },
  })

  useEffect(() => {
    if (!selectedMessage || !selectedMessageQuery.data) {
      return
    }
    const selectionKey = `${selectedMessage.sourceId}:${selectedMessage.messageId}`
    if (lastAutoSeenKeyRef.current === selectionKey) {
      return
    }
    lastAutoSeenKeyRef.current = selectionKey

    if (selectedMessageQuery.data.isRead) {
      return
    }

    actions.markRead({
      conversationId: selectedMessage.conversationId,
      sourceId: selectedMessage.sourceId,
      messageId: selectedMessage.messageId,
      isFlagged: selectedMessageQuery.data.isFlagged,
      isRead: selectedMessageQuery.data.isRead,
      keywords: selectedMessageQuery.data.keywords,
    })
  }, [actions, selectedMessage, selectedMessageQuery.data])

  const handleToggleFlag = useCallback(() => {
    if (!selectedMessage) {
      return
    }
    actions.toggleFlag({
      conversationId: selectedMessage.conversationId,
      sourceId: selectedMessage.sourceId,
      messageId: selectedMessage.messageId,
      isFlagged: selectedMessageQuery.data?.isFlagged ?? false,
      isRead: selectedMessageQuery.data?.isRead,
      keywords: selectedMessageQuery.data?.keywords,
    })
  }, [actions, selectedMessage, selectedMessageQuery.data])

  const handleArchive = useCallback(() => {
    if (!selectedMessage) {
      return
    }
    actions.archive({
      sourceId: selectedMessage.sourceId,
      messageId: selectedMessage.messageId,
    })
  }, [actions, selectedMessage])

  const handleTrash = useCallback(() => {
    if (!selectedMessage) {
      return
    }
    actions.trash({
      sourceId: selectedMessage.sourceId,
      messageId: selectedMessage.messageId,
    })
  }, [actions, selectedMessage])

  const handleOpenTagEditor = useCallback(() => {
    if (!selectedMessage) {
      return
    }
    setIsTagEditorOpen(true)
  }, [selectedMessage])

  const handleOpenFocusedMessage = useCallback(() => {
    if (!selectedMessage) {
      return
    }
    setActiveSurface(messageSurfaceFromSelection(selectedMessage))
  }, [selectedMessage])

  const resolveComposeSourceId = useCallback(() => {
    return (
      selectedMessage?.sourceId ??
      (effectiveView?.kind === 'source-mailbox'
        ? effectiveView.sourceId
        : null) ??
      enabledAccounts[0]?.id ??
      null
    )
  }, [effectiveView, enabledAccounts, selectedMessage])

  const handleCompose = useCallback(() => {
    const sourceId = resolveComposeSourceId()
    if (!sourceId) {
      setActiveSurface(settingsSurface({ category: 'accounts' }))
      return
    }
    setComposeIntent({ kind: 'new', sourceId })
  }, [resolveComposeSourceId])

  const handleReply = useCallback(() => {
    if (!selectedMessage) {
      return
    }
    setComposeIntent({
      kind: 'reply',
      sourceId: selectedMessage.sourceId,
      messageId: selectedMessage.messageId,
    })
  }, [selectedMessage])

  const handleClearSelectedMessage = useCallback(() => {
    setSelectedMessage(null)
  }, [])

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      const target = event.target as HTMLElement
      const isTypingTarget =
        target.tagName === 'INPUT' ||
        target.tagName === 'TEXTAREA' ||
        target.isContentEditable

      if (effectiveSurface !== null) {
        return
      }

      if (
        (event.metaKey || event.ctrlKey) &&
        (event.key === 'k' || event.key === 'K')
      ) {
        event.preventDefault()
        setIsCommandPaletteOpen(true)
        return
      }
      if ((event.metaKey || event.ctrlKey) && event.key === ',') {
        event.preventDefault()
        setActiveSurface(settingsSurface())
        return
      }
      if (
        (event.metaKey || event.ctrlKey) &&
        (event.key === 'n' || event.key === 'N')
      ) {
        event.preventDefault()
        handleCompose()
        return
      }
      if (
        (event.metaKey || event.ctrlKey) &&
        (event.key === 'r' || event.key === 'R')
      ) {
        event.preventDefault()
        handleReply()
        return
      }
      if (
        (event.metaKey || event.ctrlKey) &&
        event.shiftKey &&
        event.key.toLowerCase() === 'l'
      ) {
        event.preventDefault()
        if (selectedMessage) {
          handleToggleFlag()
        }
        return
      }
      if (isTypingTarget) return
      if (
        event.key === 'Escape' &&
        !isSettingsSurfaceOpen &&
        !isCommandPaletteOpen &&
        !showShortcuts &&
        composeIntent === null &&
        effectiveSurface === null
      ) {
        if (selectedMessage) {
          event.preventDefault()
          handleClearSelectedMessage()
          return
        }
        if (searchQuery.trim()) {
          event.preventDefault()
          setSearchQuery('')
          return
        }
      }
      if (event.key === '?') {
        event.preventDefault()
        setShowShortcuts((prev) => !prev)
        return
      }
      if (event.key === '/') {
        event.preventDefault()
        setIsCommandPaletteOpen(true)
        return
      }
      if (event.key.toLowerCase() === 'l' && selectedMessage) {
        event.preventDefault()
        handleOpenTagEditor()
        return
      }
      if (
        event.key.toLowerCase() === 'o' &&
        selectedMessage &&
        !isSettingsSurfaceOpen &&
        !isCommandPaletteOpen &&
        !showShortcuts &&
        composeIntent === null &&
        !isTagEditorOpen &&
        effectiveSurface === null
      ) {
        event.preventDefault()
        handleOpenFocusedMessage()
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [
    composeIntent,
    effectiveSurface,
    handleClearSelectedMessage,
    handleCompose,
    handleOpenFocusedMessage,
    handleReply,
    handleToggleFlag,
    handleOpenTagEditor,
    isCommandPaletteOpen,
    isTagEditorOpen,
    searchQuery,
    selectedMessage,
    isSettingsSurfaceOpen,
    showShortcuts,
  ])

  const applySearchQuery = useCallback((query: string, append?: boolean) => {
    setSearchQuery((previousQuery) => {
      const candidate =
        append && previousQuery ? `${previousQuery} ${query}` : query
      const normalized = normalizeValidAppliedSearchQuery(candidate)
      return normalized === null ? previousQuery : normalized
    })
  }, [])

  const handleSearch = useCallback(
    (query: string, append?: boolean) => {
      applySearchQuery(query, append)
    },
    [applySearchQuery],
  )

  const handleOpenSettings = useCallback(
    (
      category?: SettingsSurfaceCategory,
      options?: { accountId?: string | null; smartMailboxId?: string | null },
    ) => {
      setActiveSurface(
        settingsSurface({
          category,
          accountId: options?.accountId,
          smartMailboxId: options?.smartMailboxId,
        }),
      )
      setIsCommandPaletteOpen(false)
    },
    [],
  )

  const handleApplySearch = useCallback(
    (query: string) => {
      applySearchQuery(query)
    },
    [applySearchQuery],
  )

  const handlePreviewSearch = useCallback((query: string) => {
    setSearchQuery((current) => {
      const normalized = normalizeValidAppliedSearchQuery(query)
      return normalized === null || current === normalized
        ? current
        : normalized
    })
  }, [])

  const handleRejectSearchPreview = useCallback(() => {
    setSearchQuery('')
  }, [])

  function handleSelectMessage(message: MessageSummary) {
    setSelectedMessage({
      conversationId: message.conversationId,
      sourceId: message.sourceId,
      messageId: message.id,
    })
  }

  function handleSelectMessageRef(selection: MailSelection) {
    setSelectedMessage(selection)
  }

  function handleSelectSmartMailbox(smartMailboxId: string, name: string) {
    setSelectedView({ kind: 'smart-mailbox', id: smartMailboxId, name })
    setSelectedMessage(null)
  }

  function handleSelectSourceMailbox(
    sourceId: string,
    mailboxId: string,
    name: string,
  ) {
    setSelectedView({ kind: 'source-mailbox', sourceId, mailboxId, name })
    setSelectedMessage(null)
  }

  function handleSelectTag(tag: string) {
    const normalizedTag = tag.trim()
    if (!normalizedTag || normalizedTag.startsWith('$')) {
      return
    }
    applySearchQuery(`tag:${normalizedTag}`)
    setSelectedMessage(null)
  }

  if (isLoading) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3">
        <Loader2 size={24} className="animate-spin text-muted-foreground" />
        <p className="text-sm text-muted-foreground">Setting up...</p>
      </div>
    )
  }

  return (
    <div className="flex h-full flex-col overflow-hidden">
      <ActionBar
        isDarkMode={theme.resolvedMode === 'dark'}
        isFlagged={selectedMessageQuery.data?.isFlagged ?? false}
        isMessageSelected={selectedMessage !== null}
        isSettingsOpen={isSettingsSurfaceOpen}
        searchQuery={searchQuery}
        onArchive={handleArchive}
        onClearSearch={() => {
          setSearchQuery('')
        }}
        onCompose={handleCompose}
        onOpenCommandPalette={() => setIsCommandPaletteOpen(true)}
        onOpenFocusedMessage={handleOpenFocusedMessage}
        onPlaceholderAction={handlePlaceholderAction}
        onReply={handleReply}
        onShowShortcuts={() => setShowShortcuts(true)}
        onTag={handleOpenTagEditor}
        onToggleFlag={handleToggleFlag}
        onToggleSettings={() => {
          setActiveSurface((surface) =>
            surface?.kind === 'settings' ? null : settingsSurface(),
          )
        }}
        onToggleTheme={handleToggleTheme}
        onTrash={handleTrash}
      />
      {actions.errorMessage && (
        <div className="flex items-center gap-3 border-b border-destructive/20 bg-destructive/5 px-3 py-2 text-sm text-destructive">
          <span className="min-w-0 flex-1">{actions.errorMessage}</span>
          <button
            type="button"
            aria-label="Dismiss error"
            className="ph-focus-ring flex size-6 shrink-0 items-center justify-center rounded-md text-destructive/70 transition-colors hover:bg-destructive/10 hover:text-destructive"
            onClick={actions.clearError}
          >
            <X size={14} strokeWidth={1.8} />
          </button>
        </div>
      )}

      {/* Main content */}
      <ResizablePanelGroup
        orientation="horizontal"
        defaultLayout={shellDefaultLayout}
        onLayoutChanged={onShellLayoutChanged}
        className="min-h-0 flex-1"
      >
        <ResizablePanel
          id="sidebar"
          defaultSize="210px"
          minSize="190px"
          maxSize="420px"
          groupResizeBehavior="preserve-pixel-size"
        >
          <Sidebar
            selectedView={effectiveView}
            onOpenAccountSettings={(sourceId) =>
              handleOpenSettings('accounts', { accountId: sourceId })
            }
            onOpenSmartMailboxSettings={(smartMailboxId) =>
              handleOpenSettings('mailboxes', { smartMailboxId })
            }
            onSelectSmartMailbox={handleSelectSmartMailbox}
            onSelectSourceMailbox={handleSelectSourceMailbox}
            onSelectTag={handleSelectTag}
            onSyncSource={(sourceId) => syncSourceMutation.mutate(sourceId)}
          />
        </ResizablePanel>
        <ResizableHandle />
        <ResizablePanel
          id="mail-content"
          minSize="360px"
          groupResizeBehavior="preserve-relative-size"
        >
          <ResizablePanelGroup
            orientation="horizontal"
            defaultLayout={messageDefaultLayout}
            onLayoutChanged={onMessageLayoutChanged}
            className="h-full min-h-0"
          >
            <ResizablePanel
              id="message-list"
              defaultSize="420px"
              minSize="360px"
              maxSize={isMessageDetailOpen ? '960px' : undefined}
            >
              <MessageList
                selectedView={effectiveView}
                selection={selectedMessage}
                onSelectMessage={handleSelectMessageRef}
                onClearSelection={handleClearSelectedMessage}
                onClearSearchQuery={handleRejectSearchPreview}
                actions={actions}
                searchQuery={searchQuery}
                preparedSearchQuery={preparedSearchQuery}
              />
            </ResizablePanel>
            {isMessageDetailOpen && (
              <>
                <ResizableHandle />
                <ResizablePanel id="message-detail" minSize="300px">
                  <MessageDetail
                    selection={selectedMessage}
                    onSelectMessage={handleSelectMessage}
                    onSearch={handleSearch}
                  />
                </ResizablePanel>
              </>
            )}
          </ResizablePanelGroup>
        </ResizablePanel>
      </ResizablePanelGroup>

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
          onPreviewSearch={handlePreviewSearch}
          onRejectSearchPreview={handleRejectSearchPreview}
          onReply={handleReply}
          onSelectMessage={handleSelectMessage}
          onSelectSmartMailbox={handleSelectSmartMailbox}
          onSelectSourceMailbox={handleSelectSourceMailbox}
          onToggleFlag={handleToggleFlag}
        />
      )}

      {showShortcuts && (
        <ShortcutReference onClose={() => setShowShortcuts(false)} />
      )}
      {isTagEditorOpen && selectedMessageQuery.data && (
        <TagEditor
          actions={actions}
          knownTags={sidebar?.tags ?? []}
          message={selectedMessageQuery.data}
          onClose={() => setIsTagEditorOpen(false)}
        />
      )}
      {composeIntent && (
        <ComposeOverlay
          intent={composeIntent}
          onClose={() => setComposeIntent(null)}
        />
      )}
      <SurfaceHost
        surface={effectiveSurface}
        accounts={accounts}
        activeAccountId={focusedSourceId}
        canClose={!shouldForceSettings}
        onClose={() => setActiveSurface(null)}
        onSettingsActiveAccountChange={() => {
          setSelectedView(DEFAULT_VIEW)
          setSelectedMessage(null)
        }}
        onSearch={handleSearch}
        onSelectMessage={handleSelectMessage}
      />
    </div>
  )
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
            className: 'font-sans text-sm',
          }}
        />
      </QueryClientProvider>
    </DesignThemeProvider>
  )
}
