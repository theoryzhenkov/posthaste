import { useEffect } from 'react'

import { undoLogger } from '@/logger'
import { isEditableKeyboardTarget } from '@/components/keyboard/inputTargets'
import type { MailSelection } from '@/mailState'
import type { SurfaceDescriptor } from '@/surfaces'

export function useGlobalMailShortcuts({
  effectiveSurface,
  isCommandPaletteOpen,
  isComposeOpen,
  isSettingsSurfaceOpen,
  isShortcutReferenceOpen,
  isTagEditorOpen,
  searchQuery,
  selectedMessage,
  onClearSearchQuery,
  onClearSelectedMessage,
  onCompose,
  onOpenCommandPalette,
  onOpenFocusedMessage,
  onOpenSettings,
  onOpenTagEditor,
  onRedo,
  onReply,
  onToggleFlag,
  onToggleShortcuts,
  onUndo,
}: {
  effectiveSurface: SurfaceDescriptor | null
  isCommandPaletteOpen: boolean
  isComposeOpen: boolean
  isSettingsSurfaceOpen: boolean
  isShortcutReferenceOpen: boolean
  isTagEditorOpen: boolean
  searchQuery: string
  selectedMessage: MailSelection | null
  onClearSearchQuery: () => void
  onClearSelectedMessage: () => void
  onCompose: () => void
  onOpenCommandPalette: () => void
  onOpenFocusedMessage: () => void
  onOpenSettings: () => void
  onOpenTagEditor: () => void
  onRedo: () => void
  onReply: () => void
  onToggleFlag: () => void
  onToggleShortcuts: () => void
  onUndo: () => void
}) {
  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      const isTypingTarget = isEditableKeyboardTarget(event.target)

      if (effectiveSurface !== null) {
        return
      }

      if (
        (event.metaKey || event.ctrlKey) &&
        (event.key === 'k' || event.key === 'K')
      ) {
        event.preventDefault()
        onOpenCommandPalette()
        return
      }
      if ((event.metaKey || event.ctrlKey) && event.key === ',') {
        event.preventDefault()
        onOpenSettings()
        return
      }
      if (
        (event.metaKey || event.ctrlKey) &&
        (event.key === 'n' || event.key === 'N')
      ) {
        event.preventDefault()
        onCompose()
        return
      }
      if (
        (event.metaKey || event.ctrlKey) &&
        (event.key === 'r' || event.key === 'R')
      ) {
        event.preventDefault()
        onReply()
        return
      }
      if (
        (event.metaKey || event.ctrlKey) &&
        event.shiftKey &&
        event.key.toLowerCase() === 'l'
      ) {
        event.preventDefault()
        if (selectedMessage) {
          onToggleFlag()
        }
        return
      }
      if (isTypingTarget) return
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'z') {
        // Undo/redo only act on the mail surface; don't hijack the chord while
        // an overlay or editor owns input (native undo wins there).
        if (
          isComposeOpen ||
          isCommandPaletteOpen ||
          isSettingsSurfaceOpen ||
          isShortcutReferenceOpen ||
          isTagEditorOpen
        ) {
          return
        }
        event.preventDefault()
        undoLogger.debug(
          {
            key: event.key,
            shiftKey: event.shiftKey,
            repeat: event.repeat,
            ctrlKey: event.ctrlKey,
            metaKey: event.metaKey,
          },
          'undo/redo keyboard shortcut fired',
        )
        if (event.shiftKey) {
          onRedo()
        } else {
          onUndo()
        }
        return
      }
      if (
        event.key === 'Escape' &&
        !isSettingsSurfaceOpen &&
        !isCommandPaletteOpen &&
        !isShortcutReferenceOpen &&
        !isComposeOpen &&
        effectiveSurface === null
      ) {
        if (selectedMessage) {
          event.preventDefault()
          onClearSelectedMessage()
          return
        }
        if (searchQuery.trim()) {
          event.preventDefault()
          onClearSearchQuery()
          return
        }
      }
      if (event.key === '?') {
        event.preventDefault()
        onToggleShortcuts()
        return
      }
      if (event.key === '/') {
        event.preventDefault()
        onOpenCommandPalette()
        return
      }
      if (event.key.toLowerCase() === 'l' && selectedMessage) {
        event.preventDefault()
        onOpenTagEditor()
        return
      }
      if (
        event.key.toLowerCase() === 'o' &&
        selectedMessage &&
        !isSettingsSurfaceOpen &&
        !isCommandPaletteOpen &&
        !isShortcutReferenceOpen &&
        !isComposeOpen &&
        !isTagEditorOpen &&
        effectiveSurface === null
      ) {
        event.preventDefault()
        onOpenFocusedMessage()
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [
    effectiveSurface,
    isCommandPaletteOpen,
    isComposeOpen,
    isSettingsSurfaceOpen,
    isShortcutReferenceOpen,
    isTagEditorOpen,
    onClearSearchQuery,
    onClearSelectedMessage,
    onCompose,
    onOpenCommandPalette,
    onOpenFocusedMessage,
    onOpenSettings,
    onOpenTagEditor,
    onRedo,
    onReply,
    onToggleFlag,
    onToggleShortcuts,
    onUndo,
    searchQuery,
    selectedMessage,
  ])
}
