import { useCallback, type Dispatch, type SetStateAction } from 'react'
import { toast } from 'sonner'

import type {
  AccountOverview,
  MessageDetail,
  MessageSummary,
} from '@/api/types'
import type { SidebarSelection } from '@/components/Sidebar'
import { useComposeIntent } from '@/hooks/useComposeIntent'
import type { useEmailActions } from '@/hooks/useEmailActions'
import { openFocusedSurface } from '@/hooks/useSurfaceRouting'
import type { MailSelection } from '@/mailState'
import { normalizeValidAppliedSearchQuery } from '@/searchQuery'
import {
  accountSettingsSurface,
  messageSurfaceFromSelection,
  settingsCategorySurface,
  settingsSurface,
  smartMailboxSettingsSurface,
  type SettingsSurfaceCategory,
  type SurfaceDescriptor,
} from '@/surfaces'
import { toggleSettingsSurface } from './mailOverlayActions'

export function useMailClientHandlers(input: {
  actions: ReturnType<typeof useEmailActions>
  effectiveSurface: SurfaceDescriptor | null
  effectiveView: SidebarSelection | null
  enabledAccounts: AccountOverview[]
  selectedMessage: MailSelection | null
  selectedMessageData: MessageDetail | undefined
  setIsCommandPaletteOpen: (open: boolean) => void
  setIsTagEditorOpen: (open: boolean) => void
  setSearchQuery: Dispatch<SetStateAction<string>>
  setSelectedMessage: Dispatch<SetStateAction<MailSelection | null>>
  setSelectedView: Dispatch<SetStateAction<SidebarSelection | null>>
  setShowShortcuts: Dispatch<SetStateAction<boolean>>
}) {
  const {
    actions,
    effectiveSurface,
    effectiveView,
    enabledAccounts,
    selectedMessage,
    selectedMessageData,
    setIsCommandPaletteOpen,
    setIsTagEditorOpen,
    setSearchQuery,
    setSelectedMessage,
    setSelectedView,
    setShowShortcuts,
  } = input

  const applySearchQuery = useCallback(
    (query: string, append?: boolean) => {
      setSearchQuery((previousQuery) => {
        const candidate =
          append && previousQuery ? `${previousQuery} ${query}` : query
        const normalized = normalizeValidAppliedSearchQuery(candidate)
        return normalized === null ? previousQuery : normalized
      })
    },
    [setSearchQuery],
  )
  const handleMissingComposeSource = useCallback(() => {
    openFocusedSurface(settingsCategorySurface('accounts'))
  }, [])
  const compose = useComposeIntent({
    enabledAccounts,
    onMissingSource: handleMissingComposeSource,
    selectedMessage,
    selectedView: effectiveView,
  })

  const handleToggleFlag = useCallback(() => {
    if (!selectedMessage) return
    actions.toggleFlag({
      conversationId: selectedMessage.conversationId,
      sourceId: selectedMessage.sourceId,
      messageId: selectedMessage.messageId,
      isFlagged: selectedMessageData?.isFlagged ?? false,
      isRead: selectedMessageData?.isRead,
      keywords: selectedMessageData?.keywords,
    })
  }, [actions, selectedMessage, selectedMessageData])
  const handleArchive = useCallback(() => {
    if (selectedMessage) {
      actions.archive(selectedMessage)
    }
  }, [actions, selectedMessage])
  const handleTrash = useCallback(() => {
    if (selectedMessage) {
      actions.trash(selectedMessage)
    }
  }, [actions, selectedMessage])

  return {
    closeCompose: compose.closeCompose,
    composeIntent: compose.composeIntent,
    handleApplySearch: (query: string) => applySearchQuery(query),
    handleArchive,
    handleClearSelectedMessage: () => setSelectedMessage(null),
    handleCloseCommandPalette: () => setIsCommandPaletteOpen(false),
    handleCompose: compose.openCompose,
    handleEditDraft: compose.editSelectedDraft,
    handleForward: compose.forwardSelectedMessage,
    handleOpenCommandPalette: () => setIsCommandPaletteOpen(true),
    handleOpenFocusedMessage: () => {
      if (selectedMessage) {
        openFocusedSurface(messageSurfaceFromSelection(selectedMessage))
      }
    },
    handleOpenSettings: (
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
    handleOpenSettingsShortcut: () => openFocusedSurface(settingsSurface()),
    handleOpenTagEditor: () => {
      if (selectedMessage) setIsTagEditorOpen(true)
    },
    handlePlaceholderAction: (label: string) => {
      toast(`${label} is not available yet.`)
    },
    handleRejectSearchPreview: () => setSearchQuery(''),
    handleReply: compose.replyToSelectedMessage,
    handleReplyAll: compose.replyAllToSelectedMessage,
    handleSearch: applySearchQuery,
    handleSelectMessage: (message: MessageSummary) => {
      setSelectedMessage({
        conversationId: message.conversationId,
        sourceId: message.sourceId,
        messageId: message.id,
      })
    },
    handleSelectMessageRef: setSelectedMessage,
    handleSelectSmartMailbox: (smartMailboxId: string, name: string) => {
      setSelectedView({ kind: 'smart-mailbox', id: smartMailboxId, name })
      setSelectedMessage(null)
    },
    handleSelectSourceMailbox: (
      sourceId: string,
      mailboxId: string,
      name: string,
    ) => {
      setSelectedView({ kind: 'source-mailbox', sourceId, mailboxId, name })
      setSelectedMessage(null)
    },
    handleSelectTag: (tag: string) => {
      const normalizedTag = tag.trim()
      if (!normalizedTag || normalizedTag.startsWith('$')) return
      applySearchQuery(`tag:${normalizedTag}`)
      setSelectedMessage(null)
    },
    handleShowShortcuts: () => setShowShortcuts(true),
    handleToggleFlag,
    handleToggleSettings: () => toggleSettingsSurface({ effectiveSurface }),
    handleToggleShortcuts: () => setShowShortcuts((prev) => !prev),
    handleTrash,
  }
}
