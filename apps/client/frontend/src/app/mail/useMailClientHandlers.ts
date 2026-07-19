import { useCallback, type Dispatch, type SetStateAction } from 'react'
import { toast } from 'sonner'

import type { AccountRow, MessageSummary } from '@/gen'
import type { SidebarSelection } from '@/data/models/selection'
import { useComposeIntent } from '@/data/hooks/useComposeIntent'
import type { useEmailActions } from '@/data/hooks/useEmailActions'
import { openFocusedSurface } from '../host/navigation'
import type { MailSelection } from '@/data/models/selection'
import { parseSearchQuery } from '@/domain/search'
import {
  accountSettingsSurface,
  messageSurfaceFromSelection,
  settingsCategorySurface,
  settingsSurface,
  smartMailboxSettingsSurface,
  type SettingsSurfaceCategory,
  type SurfaceDescriptor,
} from '@/domain/surface'
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
  /** The list's `j`/`k` SELECTION cursor — where the next list action lands. */
  setSelectedMessage: Dispatch<SetStateAction<MailSelection | null>>
  /** The ACTIVE (opened) message the reader pane shows. Opening always aligns
   *  the cursor with it; moving the cursor afterwards leaves it in place. */
  setOpenedMessage: Dispatch<SetStateAction<MailSelection | null>>
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
    setOpenedMessage,
    setSelectedView,
    setShowShortcuts,
  } = input

  const applySearchQuery = useCallback(
    (query: string, append?: boolean) => {
      setSearchQuery((previousQuery) => {
        const candidate =
          append && previousQuery ? `${previousQuery} ${query}` : query
        const normalized = parseSearchQuery(candidate)
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

  const handleDiscardDraft = useCallback(() => {
    if (selectedMessage) {
      actions.discardDraft({
        ...selectedMessage,
        draftId: selectedMessageData?.draftId,
      })
    }
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
    // Full clear — the empty-list path (no rows left to anchor on).
    handleClearSelectedMessage: () => {
      setSelectedMessage(null)
      setOpenedMessage(null)
    },
    // Escape: close the reader; the cursor stays as the muted anchor that
    // shows where `j`/`k` resumes.
    handleCloseReader: () => setOpenedMessage(null),
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
    // OPEN a message (row click, palette result, thread switcher, context
    // menu's Open): the reader shows it and the cursor aligns with it.
    handleSelectMessage: (message: MessageSummary) => {
      const selection = {
        conversationId: message.conversationId,
        sourceId: message.sourceId,
        messageId: message.id,
      }
      setSelectedMessage(selection)
      setOpenedMessage(selection)
    },
    // Move the SELECTION cursor only (`j`/`k`): the reader keeps the message
    // it has open, so cursor and active row may diverge.
    handleSelectMessageRef: setSelectedMessage,
    // OPEN by reference — the selection-shaped twin of handleSelectMessage.
    handleOpenMessageRef: (selection: MailSelection) => {
      setSelectedMessage(selection)
      setOpenedMessage(selection)
    },
    handleSelectSmartMailbox: (smartMailboxId: string, name: string) => {
      setSelectedView({ kind: 'smart-mailbox', id: smartMailboxId, name })
      setSelectedMessage(null)
      setOpenedMessage(null)
    },
    handleSelectSourceMailbox: (
      sourceId: string,
      mailboxId: string,
      name: string,
    ) => {
      setSelectedView({ kind: 'source-mailbox', sourceId, mailboxId, name })
      setSelectedMessage(null)
      setOpenedMessage(null)
    },
    handleShowShortcuts: () => setShowShortcuts(true),
    handleToggleSettings: () => toggleSettingsSurface({ effectiveSurface }),
    handleToggleShortcuts: () => setShowShortcuts((prev) => !prev),
    handleSnooze,
  }
}
