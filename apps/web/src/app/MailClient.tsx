import { useMutation, useQuery } from '@tanstack/react-query'
import { useCallback, useEffect, useMemo, useState } from 'react'
import { Loader2 } from 'lucide-react'
import { toast } from 'sonner'

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
import { KeyboardController } from '@/components/keyboard/KeyboardController'
import { useGotoNavigation } from '@/components/keyboard/useGotoNavigation'
import { useMailboxRole, useSmartMailboxRole } from '@/hooks/useMailboxRole'
import { useMailLayoutPersistence } from '@/hooks/useMailLayoutPersistence'
import { closeWebSurface, useEffectiveSurface } from '@/hooks/useSurfaceRouting'
import {
  appReadinessStateFromAccountsQuery,
  LAB_READINESS_STATES,
} from '@/labReadiness'
import { useMailNavigationReadBootstrap } from '@/mailboxNavigationReadModels'
import { mailKeys, type MailSelection } from '@/mailState'
import { OnboardingTour } from '@/onboarding/OnboardingTour'
import { useOnboardingNeeded } from '@/onboarding/store'
import { useUndoRedo } from '@/hooks/useUndoRedo'
import { RevLogMirrors } from '@/hooks/RevLogMirrors'
import { queryClient } from '@/app/queryClient'
import { queryKeys } from '@/queryKeys'
import { consumeRepairCompletion } from '@/repairFeedback'
import { runtimeMutations } from '@/runtime/mutations'
import { runtimeViews } from '@/runtime/views'
import { conversationViewQuery, prepareServerSearchQuery } from '@/searchQuery'
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
 * @spec docs/ui/L0#navigation-model
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
  const onboardingNeeded = useOnboardingNeeded()
  useRepairCompletionToast()
  const undoRedo = useUndoRedo()
  const actions = useEmailActions({ undo: undoRedo.undo })

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
  // Phase 2: mirror the server-authoritative RevLog view for EVERY enabled
  // account so undo/redo history converges cross-device per-account (the
  // RECEIVE half of cursor sync; `useUndoRedo` sends the `revCursor`). The
  // global Ctrl+Z merges the per-account partitions by `createdAt` in the store.
  // @spec docs/eph/DESIGN-L2-undo-redo-revlog-contract
  const enabledAccountIds = useMemo(
    () => enabledAccounts.map((account) => account.id),
    [enabledAccounts],
  )
  const hasEnabledSources = enabledAccounts.length > 0
  const effectiveView = hasEnabledSources
    ? (selectedView ?? DEFAULT_VIEW)
    : null
  const sourceRole = useMailboxRole(
    effectiveView?.kind === 'source-mailbox' ? effectiveView.sourceId : null,
    effectiveView?.kind === 'source-mailbox' ? effectiveView.mailboxId : null,
  )
  const smartRole = useSmartMailboxRole(
    effectiveView?.kind === 'smart-mailbox' ? effectiveView.id : null,
  )
  const viewRole =
    effectiveView?.kind === 'smart-mailbox' ? smartRole : sourceRole
  const hasAccountsError =
    accountsQuery.isError || mailNavigationBootstrap.isError
  const isLoading = accountsQuery.isLoading || mailNavigationBootstrap.isLoading
  const hasLoadedAccounts =
    accountsQuery.isSuccess || mailNavigationBootstrap.isSuccess
  const { effectiveSurface, isSettingsSurfaceOpen } = useEffectiveSurface({
    routeSurface,
  })

  useDesktopCloseRequest(effectiveSurface)

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
  })

  const gotoNavigation = useGotoNavigation({
    effectiveView,
    onSelectSmartMailbox: handlers.handleSelectSmartMailbox,
    onSelectSourceMailbox: handlers.handleSelectSourceMailbox,
  })

  const appReadinessState = appReadinessStateFromAccountsQuery({
    isLoading,
    isSuccess: hasLoadedAccounts,
    isError: hasAccountsError,
  })

  if (isLoading) {
    return <MailClientLoading />
  }

  const showOnboarding =
    onboardingNeeded && hasLoadedAccounts && !hasAccountsError

  return (
    <>
      <RevLogMirrors accountIds={enabledAccountIds} />
      {showOnboarding && <OnboardingTour />}
      <KeyboardController
        effectiveSurfaceOpen={effectiveSurface !== null}
        overlayOwnsInput={
          isCommandPaletteOpen ||
          handlers.composeIntent !== null ||
          showShortcuts ||
          isTagEditorOpen
        }
        hasSelectedMessage={selectedMessage !== null}
        hasSearchQuery={searchQuery.trim().length > 0}
        onOpenCommandPalette={handlers.handleOpenCommandPalette}
        onOpenSettings={handlers.handleOpenSettingsShortcut}
        onCompose={handlers.handleCompose}
        onReply={handlers.handleReply}
        onReplyAll={handlers.handleReplyAll}
        onToggleFlag={handlers.handleToggleFlag}
        onUndo={undoRedo.undo}
        onRedo={undoRedo.redo}
        onArchive={handlers.handleArchive}
        onTrash={handlers.handleTrash}
        onOpenTagEditor={handlers.handleOpenTagEditor}
        onOpenFocusedMessage={handlers.handleOpenFocusedMessage}
        onClearSelectedMessage={handlers.handleClearSelectedMessage}
        onClearSearchQuery={handlers.handleRejectSearchPreview}
        onToggleShortcuts={handlers.handleToggleShortcuts}
        onGoto={gotoNavigation.goto}
        onGotoConversation={() => {
          if (selectedMessage) {
            handlers.handleSearch(
              conversationViewQuery(selectedMessage.conversationId),
            )
          }
        }}
      >
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
          preparedSearchQuery={preparedSearchQuery}
          searchQuery={searchQuery}
          selectedMessage={selectedMessage}
          selectedMessageData={selectedMessageQuery.data}
          shellDefaultLayout={layout.shellDefaultLayout}
          showShortcuts={showShortcuts}
          tags={tagsQuery.data ?? []}
          viewRole={viewRole}
          onAddTag={handlers.handleAddTag}
          onApplySearch={handlers.handleApplySearch}
          onArchive={handlers.handleArchive}
          onSnooze={handlers.handleSnooze}
          onDiscardDraft={handlers.handleDiscardDraft}
          onEditDraft={handlers.handleEditDraft}
          onClearSearch={handlers.handleRejectSearchPreview}
          onClearSelectedMessage={handlers.handleClearSelectedMessage}
          onCloseCommandPalette={handlers.handleCloseCommandPalette}
          onCompose={handlers.handleCompose}
          onForward={handlers.handleForward}
          onReplyAll={handlers.handleReplyAll}
          onMessageLayoutChanged={layout.onMessageLayoutChanged}
          onOpenCommandPalette={handlers.handleOpenCommandPalette}
          onOpenFocusedMessage={handlers.handleOpenFocusedMessage}
          onOpenSettings={handlers.handleOpenSettings}
          onOpenTagEditor={handlers.handleOpenTagEditor}
          onPlaceholderAction={handlers.handlePlaceholderAction}
          onRejectSearchPreview={handlers.handleRejectSearchPreview}
          onRemoveTag={handlers.handleRemoveTag}
          onReply={handlers.handleReply}
          onSearch={handlers.handleSearch}
          onSelectMessage={handlers.handleSelectMessage}
          onSelectMessageRef={handlers.handleSelectMessageRef}
          onSelectSmartMailbox={handlers.handleSelectSmartMailbox}
          onSelectSourceMailbox={handlers.handleSelectSourceMailbox}
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
      </KeyboardController>
    </>
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

/** Confirm a completed local repair / factory reset once, after the relaunch. */
function useRepairCompletionToast() {
  useEffect(() => {
    const kind = consumeRepairCompletion()
    if (kind === 'factory-reset') {
      toast(
        'Posthaste was reset to a clean state. Add an account to get started.',
      )
    } else if (kind === 'repair') {
      toast('Posthaste repaired your local data and is re-syncing your mail.')
    }
  }, [])
}

function useDesktopCloseRequest(effectiveSurface: SurfaceDescriptor | null) {
  useEffect(() => {
    let unlisten: (() => void) | null = null
    let disposed = false

    void listenForDesktopCloseRequest(() => {
      if (effectiveSurface) {
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
  }, [effectiveSurface])
}

function useSyncSourceMutation() {
  return useMutation({
    mutationFn: (sourceId: string) =>
      runtimeMutations.accounts.sync({ sourceId }),
    onSuccess: async () => {
      await invalidateSyncStartedReadModels(queryClient)
      toast('Sync started')
    },
    onError: (error) => {
      toast.error(error.message)
    },
  })
}
