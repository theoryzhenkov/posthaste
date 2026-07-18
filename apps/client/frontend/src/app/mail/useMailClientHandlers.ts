import { useCallback, type Dispatch, type SetStateAction } from 'react'
import { toast } from 'sonner'

import type { AccountRow, MessageSummary } from '@/gen'
import type { SidebarSelection } from '@/components/sidebar/Sidebar'
import { SYSTEM_KEYWORDS } from '@/domain/vocabulary'
import { useComposeIntent } from '@/data/hooks/useComposeIntent'
import type { useEmailActions } from '@/data/hooks/useEmailActions'
import { openFocusedSurface } from '@/surfaces/useSurfaceRouting'
import type { MailSelection } from '@/data/models/selection'
import { normalizeValidAppliedSearchQuery } from '@/domain/searchQuery'
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
  enabledAccounts: AccountRow[]
  selectedMessage: MailSelection | null
  selectedMessageData: MessageSummary | undefined
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

  const applyTagChange = useCallback(
    (nextUserTags: (current: string[]) => string[]) => {
      if (!selectedMessage) return
      const keywords = selectedMessageData?.keywords ?? []
      const currentUserTags = keywords.filter(
        (keyword) => !keyword.startsWith('$'),
      )
      actions.setUserTags(
        {
          conversationId: selectedMessage.conversationId,
          sourceId: selectedMessage.sourceId,
          messageId: selectedMessage.messageId,
          isFlagged: selectedMessageData?.isFlagged ?? false,
          isRead: selectedMessageData?.isRead,
          keywords,
        },
        nextUserTags(currentUserTags),
      )
    },
    [actions, selectedMessage, selectedMessageData],
  )
  const handleAddTag = useCallback(
    (tag: string) => applyTagChange((current) => [...current, tag]),
    [applyTagChange],
  )
  const handleRemoveTag = useCallback(
    (tag: string) =>
      applyTagChange((current) =>
        current.filter(
          (candidate) => candidate.toLowerCase() !== tag.toLowerCase(),
        ),
      ),
    [applyTagChange],
  )

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
  const handleDiscardDraft = useCallback(() => {
    if (selectedMessage) {
      actions.discardDraft({
        ...selectedMessage,
        draftId: selectedMessageData?.draftId,
      })
    }
  }, [actions, selectedMessage, selectedMessageData])
  const handleTrash = useCallback(() => {
    if (!selectedMessage) return
    // Deleting a draft is a discard (hard delete via the draft-delete op),
    // never a trash move — keeps the keyboard/`#` path coherent with the row.
    if (selectedMessageData?.keywords.includes(SYSTEM_KEYWORDS.Draft)) {
      actions.discardDraft({
        ...selectedMessage,
        draftId: selectedMessageData?.draftId,
      })
      return
    }
    actions.trash(selectedMessage)
  }, [actions, selectedMessage, selectedMessageData])
  const handleSnooze = useCallback(
    (until: number) => {
      if (selectedMessage) {
        actions.snooze(selectedMessage, until)
      }
    },
    [actions, selectedMessage],
  )

  return {
    closeCompose: compose.closeCompose,
    composeIntent: compose.composeIntent,
    // Reopen the composer on a specific draft (the scheduled-send undo's
    // restore path — distinct from handleEditDraft's selected-message form).
    editDraft: compose.editDraft,
    handleAddTag,
    handleRemoveTag,
    handleApplySearch: (query: string) => applySearchQuery(query),
    handleArchive,
    handleClearSelectedMessage: () => setSelectedMessage(null),
    handleCloseCommandPalette: () => setIsCommandPaletteOpen(false),
    handleCompose: compose.openCompose,
    handleDiscardDraft,
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
    /** List-Unsubscribe mailto path: composer prefilled from the URI, sending
     *  as the selected message's account. The user reviews and sends. */
    handleUnsubscribeMailto: (mailtoUri: string) => {
      if (selectedMessage) {
        compose.composeMailto(selectedMessage.sourceId, mailtoUri)
      }
    },
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
    handleShowShortcuts: () => setShowShortcuts(true),
    handleToggleFlag,
    handleToggleSettings: () => toggleSettingsSurface({ effectiveSurface }),
    handleToggleShortcuts: () => setShowShortcuts((prev) => !prev),
    handleTrash,
    handleSnooze,
  }
}
