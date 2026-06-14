import { useCallback, useEffect, useRef } from 'react'

import type { MessageSummary } from '@/api/types'
import type { EmailActions } from '@/hooks/useEmailActions'
import type { MailSelection } from '@/mailState'

import { isEditableKeyboardTarget } from '../keyboard/inputTargets'
import { messageKey } from './model'

export function useMessageListNavigation({
  actions,
  currentViewKey,
  messages,
  onClearSelection,
  onSelectMessage,
  selectedKey,
  selection,
}: {
  actions: EmailActions
  currentViewKey: string
  messages: MessageSummary[]
  onClearSelection: () => void
  onSelectMessage: (message: MailSelection) => void
  selectedKey: string | null
  selection: MailSelection | null
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

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if (isEditableKeyboardTarget(event.target)) return
      if (event.metaKey || event.ctrlKey || event.altKey) return

      switch (event.key) {
        case 'j':
        case 'ArrowDown':
          event.preventDefault()
          navigateMessage(1)
          break
        case 'k':
        case 'ArrowUp':
          event.preventDefault()
          navigateMessage(-1)
          break
        case 'e':
          if (selection) {
            actions.archive({
              sourceId: selection.sourceId,
              messageId: selection.messageId,
            })
          }
          break
        case '#':
        case 'Backspace':
          if (selection) {
            actions.trash({
              sourceId: selection.sourceId,
              messageId: selection.messageId,
            })
          }
          break
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [actions, navigateMessage, selection])
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
