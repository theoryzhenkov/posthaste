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
import { lazy, Suspense, useCallback, useMemo, useState } from 'react'
import { Loader2, X } from 'lucide-react'
import { toast, Toaster } from 'sonner'
import { shouldForceAccountSettings } from './accountSetup'
import {
  fetchAccounts,
  fetchMessage,
  fetchSidebar,
  triggerSync,
} from './api/client'
import type { MessageSummary } from './api/types'
import { ActionBar } from './components/ActionBar'
import { MessageDetail } from './components/MessageDetail'
import { MessageList } from './components/MessageList'
import { ShortcutReference } from './components/ShortcutReference'
import { Sidebar, type SidebarSelection } from './components/Sidebar'
import { TagEditor } from './components/TagEditor'
import { DesignThemeProvider } from './components/ThemeProvider'
import { isTauriRuntime } from './desktop'
import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from './components/ui/resizable'
import { useAutoMarkRead } from './hooks/useAutoMarkRead'
import { useComposeIntent } from './hooks/useComposeIntent'
import { useDaemonEvents } from './hooks/useDaemonEvents'
import { useDesignTheme } from './hooks/useDesignTheme'
import { useEmailActions } from './hooks/useEmailActions'
import { useGlobalMailShortcuts } from './hooks/useGlobalMailShortcuts'
import { useMailLayoutPersistence } from './hooks/useMailLayoutPersistence'
import {
  closeWebSurface,
  openFocusedSurface,
  useEffectiveSurface,
  useRouteSurface,
} from './hooks/useSurfaceRouting'
import { mailKeys, type MailSelection } from './mailState'
import { queryKeys } from './queryKeys'
import {
  accountSettingsSurface,
  messageSurfaceFromSelection,
  settingsCategorySurface,
  settingsSurface,
  smartMailboxSettingsSurface,
  type SettingsSurfaceCategory,
  type SurfaceDescriptor,
} from './surfaces'
import {
  normalizeValidAppliedSearchQuery,
  prepareServerSearchQuery,
} from './searchQuery'

const CommandPalette = lazy(() =>
  import('./components/CommandPalette').then((module) => ({
    default: module.CommandPalette,
  })),
)
const ComposeOverlay = lazy(() =>
  import('./components/ComposeOverlay').then((module) => ({
    default: module.ComposeOverlay,
  })),
)
const SurfaceHost = lazy(() =>
  import('./components/SurfaceHost').then((module) => ({
    default: module.SurfaceHost,
  })),
)
const FocusedSurfaceDocument = lazy(() =>
  import('./components/FocusedSurface').then((module) => ({
    default: module.FocusedSurfaceDocument,
  })),
)

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

/**
 * Main mail client shell: toolbar, three-column layout, and surface host.
 *
 * Manages view selection, message selection, SSE event subscription,
 * and keyboard-accessible email actions.
 *
 * @spec docs/L1-ui#component-hierarchy
 * @spec docs/L0-ui#navigation-model
 */
function MailClient({
  routeSurface,
}: {
  routeSurface: SurfaceDescriptor | null
}) {
  const [selectedView, setSelectedView] = useState<SidebarSelection | null>(
    DEFAULT_VIEW,
  )
  const [selectedMessage, setSelectedMessage] = useState<MailSelection | null>(
    null,
  )
  const [isCommandPaletteOpen, setIsCommandPaletteOpen] = useState(false)
  const [isTagEditorOpen, setIsTagEditorOpen] = useState(false)
  const [searchQuery, setSearchQuery] = useState('')
  const [showShortcuts, setShowShortcuts] = useState(false)
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

  const {
    data: accounts = [],
    isLoading,
    isSuccess: hasLoadedAccounts,
  } = useQuery({
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
  const shouldForceSettings = shouldForceAccountSettings({
    accounts,
    accountsQuerySucceeded: hasLoadedAccounts,
  })
  const {
    effectiveSurface,
    isSettingsSurfaceOpen,
    shouldRenderForcedSettings,
  } = useEffectiveSurface({
    routeSurface,
    shouldForceSettings,
  })
  const selectedMessageQuery = useQuery({
    queryKey: selectedMessage
      ? mailKeys.message(selectedMessage.sourceId, selectedMessage.messageId)
      : ['message', null, null],
    queryFn: () =>
      fetchMessage(selectedMessage!.messageId, selectedMessage!.sourceId),
    enabled: selectedMessage !== null,
  })
  const isMessageDetailOpen = selectedMessage !== null

  useDaemonEvents()

  const {
    messageDefaultLayout,
    onMessageLayoutChanged,
    onShellLayoutChanged,
    shellDefaultLayout,
  } = useMailLayoutPersistence(isMessageDetailOpen)
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

  useAutoMarkRead(selectedMessage, selectedMessageQuery.data, actions)

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
    openFocusedSurface(messageSurfaceFromSelection(selectedMessage))
  }, [selectedMessage])

  const handleMissingComposeSource = useCallback(() => {
    openFocusedSurface(settingsCategorySurface('accounts'))
  }, [])
  const {
    closeCompose,
    composeIntent,
    forwardSelectedMessage: handleForward,
    openCompose: handleCompose,
    replyToSelectedMessage: handleReply,
  } = useComposeIntent({
    enabledAccounts,
    onMissingSource: handleMissingComposeSource,
    selectedMessage,
    selectedView: effectiveView,
  })

  const handleClearSelectedMessage = useCallback(() => {
    setSelectedMessage(null)
  }, [])
  const handleOpenCommandPalette = useCallback(() => {
    setIsCommandPaletteOpen(true)
  }, [])
  const handleCloseCommandPalette = useCallback(() => {
    setIsCommandPaletteOpen(false)
  }, [])
  const handleShowShortcuts = useCallback(() => {
    setShowShortcuts(true)
  }, [])
  const handleToggleShortcuts = useCallback(() => {
    setShowShortcuts((prev) => !prev)
  }, [])

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
      const surface = options?.accountId
        ? accountSettingsSurface(options.accountId)
        : options?.smartMailboxId
          ? smartMailboxSettingsSurface(options.smartMailboxId)
          : category
            ? settingsCategorySurface(category)
            : settingsSurface()
      openFocusedSurface(surface)
      setIsCommandPaletteOpen(false)
    },
    [],
  )
  const handleOpenSettingsShortcut = useCallback(() => {
    openFocusedSurface(settingsSurface())
  }, [])

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

  useGlobalMailShortcuts({
    effectiveSurface,
    isCommandPaletteOpen,
    isComposeOpen: composeIntent !== null,
    isSettingsSurfaceOpen,
    isShortcutReferenceOpen: showShortcuts,
    isTagEditorOpen,
    onClearSearchQuery: handleRejectSearchPreview,
    onClearSelectedMessage: handleClearSelectedMessage,
    onCompose: handleCompose,
    onOpenCommandPalette: handleOpenCommandPalette,
    onOpenFocusedMessage: handleOpenFocusedMessage,
    onOpenSettings: handleOpenSettingsShortcut,
    onOpenTagEditor: handleOpenTagEditor,
    onReply: handleReply,
    onToggleFlag: handleToggleFlag,
    onToggleShortcuts: handleToggleShortcuts,
    searchQuery,
    selectedMessage,
  })

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
        onOpenCommandPalette={handleOpenCommandPalette}
        onOpenFocusedMessage={handleOpenFocusedMessage}
        onPlaceholderAction={handlePlaceholderAction}
        onReply={handleReply}
        onShowShortcuts={handleShowShortcuts}
        onTag={handleOpenTagEditor}
        onToggleFlag={handleToggleFlag}
        onToggleSettings={() => {
          if (
            effectiveSurface?.kind === 'settings' &&
            !shouldRenderForcedSettings
          ) {
            closeWebSurface()
          } else {
            openFocusedSurface(settingsSurface())
          }
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
                    accounts={accounts}
                    sidebar={sidebar}
                    onArchive={handleArchive}
                    onForward={handleForward}
                    onReply={handleReply}
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
        <Suspense fallback={null}>
          <CommandPalette
            hasSelectedMessage={selectedMessage !== null}
            onApplySearch={handleApplySearch}
            onArchive={handleArchive}
            onClose={handleCloseCommandPalette}
            onCompose={handleCompose}
            onOpenSettings={handleOpenSettings}
            onOpenShortcuts={handleShowShortcuts}
            onPlaceholderAction={handlePlaceholderAction}
            onPreviewSearch={handlePreviewSearch}
            onRejectSearchPreview={handleRejectSearchPreview}
            onReply={handleReply}
            onSelectMessage={handleSelectMessage}
            onSelectSmartMailbox={handleSelectSmartMailbox}
            onSelectSourceMailbox={handleSelectSourceMailbox}
            onToggleFlag={handleToggleFlag}
          />
        </Suspense>
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
        <Suspense
          fallback={
            <div className="fixed inset-0 z-50 flex items-center justify-center">
              <Loader2
                size={18}
                className="animate-spin text-muted-foreground"
              />
            </div>
          }
        >
          <ComposeOverlay intent={composeIntent} onClose={closeCompose} />
        </Suspense>
      )}
      {effectiveSurface && (
        <Suspense fallback={null}>
          <SurfaceHost
            surface={effectiveSurface}
            canClose={!shouldRenderForcedSettings}
            onClose={closeWebSurface}
            onSearch={handleSearch}
          />
        </Suspense>
      )}
    </div>
  )
}

/**
 * Root App component: wraps `MailClient` in a `QueryClientProvider`.
 * @spec docs/L1-ui#component-hierarchy
 */
export default function App() {
  const routeSurface = useRouteSurface()
  const isStandaloneSurface = isTauriRuntime() && routeSurface !== null

  return (
    <DesignThemeProvider>
      <QueryClientProvider client={queryClient}>
        {isStandaloneSurface ? (
          <Suspense
            fallback={
              <div className="flex h-screen items-center justify-center bg-background text-foreground">
                <Loader2
                  size={18}
                  className="animate-spin text-muted-foreground"
                />
              </div>
            }
          >
            <FocusedSurfaceDocument surface={routeSurface} />
          </Suspense>
        ) : (
          <MailClient routeSurface={routeSurface} />
        )}
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
