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
import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from 'react'
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
import { ErrorBoundary } from './components/ErrorBoundary'
import { MessageDetail } from './components/MessageDetail'
import { MessageList } from './components/MessageList'
import { FocusedSurfaceDocument } from './components/FocusedSurface'
import {
  InvalidSurface,
  InvalidSurfaceDocument,
} from './components/InvalidSurface'
import { ShortcutReference } from './components/ShortcutReference'
import { Sidebar, type SidebarSelection } from './components/Sidebar'
import { SurfaceHost } from './components/SurfaceHost'
import { TagEditor } from './components/TagEditor'
import { DesignThemeProvider } from './components/ThemeProvider'
import { ConnectionScreen } from './connection/ConnectionScreen'
import { useActiveConnection } from './connection/connectionContext'
import { ActiveConnectionProvider } from './connection/useActiveConnection'
import {
  closeCurrentSurfaceWindow,
  isMainDesktopWindow,
  isTauriRuntime,
  listenForDesktopCloseRequest,
  toggleDevtools,
} from './desktop'
import { isDeveloperToolsEnabled } from './developerTools'
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
import { useMailboxRole } from './hooks/useMailboxRole'
import { useMailLayoutPersistence } from './hooks/useMailLayoutPersistence'
import {
  closeWebSurface,
  openFocusedSurface,
  useEffectiveSurface,
  useSurfaceRouteState,
} from './hooks/useSurfaceRouting'
import {
  appReadinessStateFromAccountsQuery,
  LAB_READINESS_STATES,
} from './labReadiness'
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
  invalidSurfaceRoute,
  routeSurface,
}: {
  invalidSurfaceRoute: string | null
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
    isError: hasAccountsError,
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
  // Mailbox role of the active view, resolved from the mailbox domain read
  // model. Drives contextual row actions and is correct regardless of how the
  // mailbox was reached — sidebar, command palette, or the default view.
  const viewRole = useMailboxRole(
    effectiveView?.kind === 'source-mailbox' ? effectiveView.sourceId : null,
    effectiveView?.kind === 'source-mailbox' ? effectiveView.mailboxId : null,
  )
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
  useEffect(() => {
    let unlisten: (() => void) | null = null
    let disposed = false

    void listenForDesktopCloseRequest(() => {
      if (effectiveSurface && !shouldRenderForcedSettings) {
        closeWebSurface()
        return
      }
      void closeCurrentSurfaceWindow()
    }).then((nextUnlisten) => {
      if (disposed) {
        nextUnlisten()
        return
      }
      unlisten = nextUnlisten
    })

    return () => {
      disposed = true
      unlisten?.()
    }
  }, [effectiveSurface, shouldRenderForcedSettings])

  const selectedMessageQuery = useQuery({
    queryKey: selectedMessage
      ? mailKeys.message(selectedMessage.sourceId, selectedMessage.messageId)
      : ['message', null, null],
    queryFn: () =>
      fetchMessage(selectedMessage!.messageId, selectedMessage!.sourceId),
    enabled: selectedMessage !== null,
  })
  const isMessageDetailOpen = selectedMessage !== null

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

  const appReadinessState = appReadinessStateFromAccountsQuery({
    isLoading,
    isSuccess: hasLoadedAccounts,
    isError: hasAccountsError,
  })

  if (isLoading) {
    return (
      <div
        className="flex h-full flex-col items-center justify-center gap-3"
        data-posthaste-state={LAB_READINESS_STATES.appLoading}
      >
        <Loader2 size={24} className="animate-spin text-muted-foreground" />
        <p className="text-sm text-muted-foreground">Setting up...</p>
      </div>
    )
  }

  return (
    <div
      className="flex h-full flex-col overflow-hidden"
      data-posthaste-state={appReadinessState}
    >
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
                viewRole={viewRole}
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
      {invalidSurfaceRoute && !shouldRenderForcedSettings && (
        <div className="fixed inset-0 z-[2300] bg-background text-foreground">
          <InvalidSurface
            route={invalidSurfaceRoute}
            onClose={closeWebSurface}
          />
        </div>
      )}
      {effectiveSurface &&
        (!invalidSurfaceRoute || shouldRenderForcedSettings) && (
          <ErrorBoundary label="surface" resetKeys={[effectiveSurface]}>
            <SurfaceHost
              surface={effectiveSurface}
              canClose={!shouldRenderForcedSettings}
              onClose={closeWebSurface}
              onSearch={handleSearch}
            />
          </ErrorBoundary>
        )}
    </div>
  )
}

function DaemonEventBridge() {
  useDaemonEvents()
  return null
}

function renderAppRootError(error: Error) {
  return (
    <div className="fixed inset-0 flex flex-col items-center justify-center gap-3 bg-background p-6 text-center">
      <p className="text-sm font-medium text-foreground">
        The app hit an unexpected error
      </p>
      <p className="max-w-md text-xs break-words text-muted-foreground">
        {error.message}
      </p>
      <button
        type="button"
        className="rounded-md border border-border px-3 py-1.5 text-sm hover:bg-muted"
        onClick={() => window.location.reload()}
      >
        Reload
      </button>
    </div>
  )
}

/**
 * Root App component: wraps `MailClient` in a `QueryClientProvider`.
 * @spec docs/L1-ui#component-hierarchy
 */
/**
 * Gate the app behind a resolvable connection. The active connection is seeded
 * synchronously to the embedded default at module load, so the bundled build
 * renders mail immediately (status `loading` → `connected` with no flash). Only
 * a true `needs-connection` (client-only build with no profile, or an
 * unreachable local/remote daemon) shows the connect screen.
 *
 * @spec docs/eph/DESIGN-L1-deployment-modes#build-modes
 */
function ConnectionGate({ children }: { children: ReactNode }) {
  const { status } = useActiveConnection()
  if (status === 'needs-connection') {
    return <ConnectionScreen />
  }
  return children
}

export default function App() {
  const routeState = useSurfaceRouteState()
  const routeSurface = routeState.kind === 'valid' ? routeState.surface : null
  const invalidSurfaceRoute =
    routeState.kind === 'invalid' ? routeState.route : null
  const isStandaloneSurface =
    isTauriRuntime() && routeState.kind !== 'none' && !isMainDesktopWindow()

  // Devtools shortcut for every window, gated by the "Developer tools" setting.
  // `toggleDevtools` is a no-op off desktop, so the listener is harmless on web.
  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if (
        (event.metaKey || event.ctrlKey) &&
        event.altKey &&
        event.code === 'KeyI' &&
        isDeveloperToolsEnabled()
      ) {
        event.preventDefault()
        void toggleDevtools()
      }
    }
    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [])

  return (
    <QueryClientProvider client={queryClient}>
      <DesignThemeProvider>
        <ActiveConnectionProvider>
          <DaemonEventBridge
            key={isStandaloneSurface ? 'standalone' : 'mail'}
          />
          <ErrorBoundary label="app-root" fallback={renderAppRootError}>
            <ConnectionGate>
              {isStandaloneSurface && routeSurface ? (
                <FocusedSurfaceDocument surface={routeSurface} />
              ) : isStandaloneSurface && invalidSurfaceRoute ? (
                <InvalidSurfaceDocument route={invalidSurfaceRoute} />
              ) : (
                <MailClient
                  invalidSurfaceRoute={invalidSurfaceRoute}
                  routeSurface={routeSurface}
                />
              )}
            </ConnectionGate>
          </ErrorBoundary>
          <Toaster
            position="bottom-center"
            toastOptions={{
              className: 'font-sans text-sm',
            }}
          />
        </ActiveConnectionProvider>
      </DesignThemeProvider>
    </QueryClientProvider>
  )
}
