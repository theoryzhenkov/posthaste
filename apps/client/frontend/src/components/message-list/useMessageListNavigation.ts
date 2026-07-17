import { useCallback, useEffect, useRef } from 'react'

import type { MessageSummary } from '@/api/types'
import type { MailSelection } from '@/data/selection'

import { useFocusedPaneHandler } from '../keyboard/usePane'
import { messageKey } from './model'

/**
 * Within-list `j`/`k` navigation, registered as the `list` pane's focused-key
 * handler. The keyboard controller owns the window listener and routes keys
 * here only when the list (or detail, which reuses it) is focused; archive and
 * trash live in the controller as selection-scoped actions.
 */
export function useMessageListNavigation({
  currentViewKey,
  messages,
  onClearSelection,
  onSelectMessage,
  selectedKey,
  onCollapseFocused,
  onExpandFocused,
}: {
  currentViewKey: string
  messages: MessageSummary[]
  onClearSelection: () => void
  onSelectMessage: (message: MailSelection) => void
  selectedKey: string | null
  /** Collapse the focused conversation node (`h`/←). Tree view only; a no-op on
   *  a leaf or already-collapsed node. Absent in flat list mode. */
  onCollapseFocused?: () => void
  /** Expand the focused conversation node (`l`/→). Tree view only. */
  onExpandFocused?: () => void
}) {
  const lastSelectedSlotRef = useRef<{ viewKey: string; index: number } | null>(
    null,
  )

  useEffect(() => {
    const index = messages.findIndex(
      (message) => messageKey(message) === selectedKey,
    )
    if (index !== -1) {
      lastSelectedSlotRef.current = { viewKey: currentViewKey, index }
    }
  }, [messages, selectedKey, currentViewKey])

  useEffect(() => {
    if (selectedKey === null) return
    if (messages.some((message) => messageKey(message) === selectedKey)) return

    const slot = lastSelectedSlotRef.current
    if (!slot || slot.viewKey !== currentViewKey) return

    if (messages.length === 0) {
      onClearSelection()
      return
    }

    const nextMessage = messages[Math.min(slot.index, messages.length - 1)]
    onSelectMessage(toSelection(nextMessage))
  }, [messages, selectedKey, currentViewKey, onSelectMessage, onClearSelection])

  const navigateMessage = useCallback(
    (direction: 1 | -1) => {
      if (messages.length === 0) return

      const currentIndex = messages.findIndex(
        (message) => messageKey(message) === selectedKey,
      )
      const rememberedSlot =
        lastSelectedSlotRef.current?.viewKey === currentViewKey
          ? lastSelectedSlotRef.current.index
          : -1
      const nextIndex = nextNavigationIndex({
        currentIndex,
        direction,
        length: messages.length,
        rememberedSlot,
      })
      if (nextIndex === null) return
      onSelectMessage(toSelection(messages[nextIndex]))
    },
    [messages, onSelectMessage, selectedKey, currentViewKey],
  )

  useFocusedPaneHandler('list', (event) => {
    if (event.metaKey || event.ctrlKey || event.altKey || event.shiftKey)
      return false
    switch (event.key) {
      case 'j':
      case 'ArrowDown':
        event.preventDefault()
        navigateMessage(1)
        return true
      case 'k':
      case 'ArrowUp':
        event.preventDefault()
        navigateMessage(-1)
        return true
      case 'h':
      case 'ArrowLeft':
        // Tree view: collapse the focused conversation (VS Code left-arrow).
        if (!onCollapseFocused) return false
        event.preventDefault()
        onCollapseFocused()
        return true
      case 'l':
      case 'ArrowRight':
        // Tree view: expand the focused conversation (VS Code right-arrow).
        if (!onExpandFocused) return false
        event.preventDefault()
        onExpandFocused()
        return true
      default:
        return false
    }
  })
}

function toSelection(message: MessageSummary): MailSelection {
  return {
    conversationId: message.conversationId,
    sourceId: message.sourceId,
    messageId: message.id,
  }
}

function nextNavigationIndex(input: {
  currentIndex: number
  direction: 1 | -1
  length: number
  rememberedSlot: number
}): number | null {
  const { currentIndex, direction, length, rememberedSlot } = input
  if (currentIndex !== -1) {
    const nextIndex = currentIndex + direction
    return nextIndex < 0 || nextIndex >= length ? null : nextIndex
  }
  if (rememberedSlot !== -1) {
    const base = direction === 1 ? rememberedSlot : rememberedSlot - 1
    return Math.min(Math.max(base, 0), length - 1)
  }
  return direction === 1 ? 0 : length - 1
}
