import { useMutation, useQuery } from '@tanstack/react-query'
import { useCallback, useEffect, useMemo, useState } from 'react'
import { Loader2 } from 'lucide-react'
import { toast } from 'sonner'

import { shouldForceAccountSettings } from '@/accountSetup'
import type { TagSummary } from '@/api/types'
import type { SidebarSelection } from '@/components/Sidebar'
import {
  closeCurrentSurfaceWindow,
  listenForDesktopCloseRequest,
} from '@/desktop'
import { invalidateSyncStartedReadModels } from '@/domainCache'
import { useAutoMarkRead } from '@/hooks/useAutoMarkRead'
import { useDesignTheme } from '@/hooks/useDesignTheme'
import { useEmailActions } from '@/hooks/useEmailActions'
import { useGlobalMailShortcuts } from '@/hooks/useGlobalMailShortcuts'
import { useMailboxRole } from '@/hooks/useMailboxRole'
import { useMailLayoutPersistence } from '@/hooks/useMailLayoutPersistence'
import { closeWebSurface, useEffectiveSurface } from '@/hooks/useSurfaceRouting'
import {
  appReadinessStateFromAccountsQuery,
  LAB_READINESS_STATES,
} from '@/labReadiness'
import { useMailNavigationReadBootstrap } from '@/mailboxNavigationReadModels'
import { mailKeys, type MailSelection } from '@/mailState'
import { useOperations } from '@/operationsContext'
import { queryClient } from '@/app/queryClient'
import { queryKeys } from '@/queryKeys'
import { runtimeMutations } from '@/runtime/mutations'
import { runtimeViews } from '@/runtime/views'
import { prepareServerSearchQuery } from '@/searchQuery'
import { type SurfaceDescriptor } from '@/surfaces'
import { MailClientView } from './MailClientView'
import { useMailClientHandlers } from './useMailClientHandlers'

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
export function MailClient({
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
  const actions = useEmailActions()
  const operations = useOperations()

  const mailNavigationBootstrap = useMailNavigationReadBootstrap()
  // Observed (not `enabled: false`): the bootstrap read seeds this cache, but
  // the query must be live so account read-model invalidations (status events,
  // sync completion) actually refetch instead of leaving the main app stale.
  const accountsQuery = useQuery({
    queryKey: queryKeys.accounts,
    queryFn: runtimeViews.accounts.list,
  })
  const accounts = useMemo(() => accountsQuery.data ?? [], [accountsQuery.data])
  const enabledAccounts = useMemo(
    () => accounts.filter((account) => account.enabled),
    [accounts],
  )
  const hasEnabledSources = enabledAccounts.length > 0
  const effectiveView = hasEnabledSources
    ? (selectedView ?? DEFAULT_VIEW)
    : null
  const viewRole = useMailboxRole(
    effectiveView?.kind === 'source-mailbox' ? effectiveView.sourceId : null,
    effectiveView?.kind === 'source-mailbox' ? effectiveView.mailboxId : null,
  )
  const hasAccountsError =
    accountsQuery.isError || mailNavigationBootstrap.isError
  const isLoading = accountsQuery.isLoading || mailNavigationBootstrap.isLoading
  const hasLoadedAccounts =
    accountsQuery.isSuccess || mailNavigationBootstrap.isSuccess
  const shouldForceSettings = shouldForceAccountSettings({
    accounts,
    accountsQuerySucceeded: hasLoadedAccounts,
  })
  const {
    effectiveSurface,
    isSettingsSurfaceOpen,
    shouldRenderForcedSettings,
  } = useEffectiveSurface({ routeSurface, shouldForceSettings })

  useDesktopCloseRequest(effectiveSurface, shouldRenderForcedSettings)

  const tagsQuery = useQuery<TagSummary[]>({
    queryKey: queryKeys.tags,
    queryFn: () => Promise.resolve([]),
    enabled: false,
  })
  const selectedMessageQuery = useQuery({
    queryKey: selectedMessage
      ? mailKeys.message(selectedMessage.sourceId, selectedMessage.messageId)
      : [...mailKeys.messageRoot, null, null],
    queryFn: () =>
      runtimeViews.mail.message(
        selectedMessage!.messageId,
        selectedMessage!.sourceId,
      ),
    enabled: selectedMessage !== null,
  })
  const isMessageDetailOpen = selectedMessage !== null
  const layout = useMailLayoutPersistence(isMessageDetailOpen)
  const syncSourceMutation = useSyncSourceMutation()

  useAutoMarkRead(selectedMessage, selectedMessageQuery.data, actions)

  const handleToggleTheme = useCallback(() => {
    theme.setMode(theme.resolvedMode === 'dark' ? 'light' : 'dark')
  }, [theme])

  const handlers = useMailClientHandlers({
    actions,
    effectiveSurface,
    effectiveView,
    enabledAccounts,
    selectedMessage,
    selectedMessageData: selectedMessageQuery.data,
    setIsCommandPaletteOpen,
    setIsTagEditorOpen,
    setSearchQuery,
    setSelectedMessage,
    setSelectedView,
    setShowShortcuts,
    shouldRenderForcedSettings,
  })

  useGlobalMailShortcuts({
    effectiveSurface,
    isCommandPaletteOpen,
    isComposeOpen: handlers.composeIntent !== null,
    isSettingsSurfaceOpen,
    isShortcutReferenceOpen: showShortcuts,
    isTagEditorOpen,
    onClearSearchQuery: handlers.handleRejectSearchPreview,
    onClearSelectedMessage: handlers.handleClearSelectedMessage,
    onCompose: handlers.handleCompose,
    onOpenCommandPalette: handlers.handleOpenCommandPalette,
    onOpenFocusedMessage: handlers.handleOpenFocusedMessage,
    onOpenSettings: handlers.handleOpenSettingsShortcut,
    onOpenTagEditor: handlers.handleOpenTagEditor,
    onRedo: operations.redo,
    onReply: handlers.handleReply,
    onToggleFlag: handlers.handleToggleFlag,
    onToggleShortcuts: handlers.handleToggleShortcuts,
    onUndo: operations.undo,
    searchQuery,
    selectedMessage,
  })

  const appReadinessState = appReadinessStateFromAccountsQuery({
    isLoading,
    isSuccess: hasLoadedAccounts,
    isError: hasAccountsError,
  })

  if (isLoading) {
    return <MailClientLoading />
  }

  return (
    <MailClientView
      actions={actions}
      appReadinessState={appReadinessState}
      closeCompose={handlers.closeCompose}
      composeIntent={handlers.composeIntent}
      effectiveSurface={effectiveSurface}
      effectiveView={effectiveView}
      invalidSurfaceRoute={invalidSurfaceRoute}
      isCommandPaletteOpen={isCommandPaletteOpen}
      isDarkMode={theme.resolvedMode === 'dark'}
      isMessageDetailOpen={isMessageDetailOpen}
      isSettingsSurfaceOpen={isSettingsSurfaceOpen}
      isTagEditorOpen={isTagEditorOpen}
      messageDefaultLayout={layout.messageDefaultLayout}
      operations={operations}
      preparedSearchQuery={preparedSearchQuery}
      searchQuery={searchQuery}
      selectedMessage={selectedMessage}
      selectedMessageData={selectedMessageQuery.data}
      shellDefaultLayout={layout.shellDefaultLayout}
      shouldRenderForcedSettings={shouldRenderForcedSettings}
      showShortcuts={showShortcuts}
      tags={tagsQuery.data ?? []}
      viewRole={viewRole}
      onApplySearch={handlers.handleApplySearch}
      onArchive={handlers.handleArchive}
      onClearSearch={handlers.handleRejectSearchPreview}
      onClearSelectedMessage={handlers.handleClearSelectedMessage}
      onCloseCommandPalette={handlers.handleCloseCommandPalette}
      onCompose={handlers.handleCompose}
      onForward={handlers.handleForward}
      onMessageLayoutChanged={layout.onMessageLayoutChanged}
      onOpenCommandPalette={handlers.handleOpenCommandPalette}
      onOpenFocusedMessage={handlers.handleOpenFocusedMessage}
      onOpenSettings={handlers.handleOpenSettings}
      onPlaceholderAction={handlers.handlePlaceholderAction}
      onRejectSearchPreview={handlers.handleRejectSearchPreview}
      onReply={handlers.handleReply}
      onSearch={handlers.handleSearch}
      onSelectMessage={handlers.handleSelectMessage}
      onSelectMessageRef={handlers.handleSelectMessageRef}
      onSelectSmartMailbox={handlers.handleSelectSmartMailbox}
      onSelectSourceMailbox={handlers.handleSelectSourceMailbox}
      onSelectTag={handlers.handleSelectTag}
      onSetTagEditorOpen={setIsTagEditorOpen}
      onShellLayoutChanged={layout.onShellLayoutChanged}
      onShowShortcuts={handlers.handleShowShortcuts}
      onSyncSource={(sourceId) => syncSourceMutation.mutate(sourceId)}
      onToggleFlag={handlers.handleToggleFlag}
      onToggleShortcuts={handlers.handleToggleShortcuts}
      onToggleSettings={handlers.handleToggleSettings}
      onToggleTheme={handleToggleTheme}
      onTrash={handlers.handleTrash}
    />
  )
}

function MailClientLoading() {
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

function useDesktopCloseRequest(
  effectiveSurface: SurfaceDescriptor | null,
  shouldRenderForcedSettings: boolean,
) {
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
}

function useSyncSourceMutation() {
  return useMutation({
    mutationFn: (sourceId: string) =>
      runtimeMutations.accounts.sync({ sourceId }),
    onSuccess: async (_result, sourceId) => {
      await invalidateSyncStartedReadModels(queryClient, sourceId)
      toast('Sync started')
    },
    onError: (error) => {
      toast.error(error.message)
    },
  })
}
