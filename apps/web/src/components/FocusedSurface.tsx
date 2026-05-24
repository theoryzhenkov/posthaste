import { useEffect } from 'react'
import { useQuery } from '@tanstack/react-query'

import { fetchAccounts, fetchSidebar } from '@/api/client'
import type { MessageSummary } from '@/api/types'
import type { SurfaceDescriptor } from '@/surfaces'
import { queryKeys } from '@/queryKeys'
import { closeCurrentSurfaceWindow, isTauriRuntime } from '@/desktop'
import { useComposeIntent } from '@/hooks/useComposeIntent'
import { useEmailActions } from '@/hooks/useEmailActions'
import { replaceFocusedSurface } from '@/hooks/useSurfaceRouting'
import { AttachmentSurface } from './AttachmentSurface'
import { ComposeOverlay } from './ComposeOverlay'
import { MessageDetail } from './MessageDetail'
import { SettingsPanel } from './SettingsPanel'

interface FocusedSurfaceProps {
  surface: SurfaceDescriptor
  canClose?: boolean
  onClose?: () => void
  onSearch?: (query: string, append?: boolean) => void
  onSelectMessage?: (message: MessageSummary) => void
}

export function FocusedSurface({
  surface,
  canClose = true,
  onClose,
  onSearch,
  onSelectMessage,
}: FocusedSurfaceProps) {
  const selectedMessage = surface.kind === 'message' ? surface.params : null
  const accountsQuery = useQuery({
    queryKey: queryKeys.accounts,
    queryFn: fetchAccounts,
    enabled: surface.kind === 'settings' || surface.kind === 'message',
  })
  const sidebarQuery = useQuery({
    queryKey: queryKeys.sidebar,
    queryFn: fetchSidebar,
    enabled: surface.kind === 'message',
  })
  const actions = useEmailActions()
  const {
    closeCompose,
    composeIntent,
    forwardSelectedMessage,
    replyToSelectedMessage,
  } = useComposeIntent({
    enabledAccounts: [],
    onMissingSource: () => {},
    selectedMessage,
    selectedView: null,
  })

  useEffect(() => {
    if (onClose) {
      return
    }

    function handleKeyDown(event: KeyboardEvent) {
      if (event.key !== 'Escape' || event.repeat || !canClose) {
        return
      }
      event.preventDefault()
      void closeCurrentSurfaceWindow()
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [canClose, onClose])

  if (surface.kind === 'settings') {
    return (
      <SettingsPanel
        accounts={accountsQuery.data ?? []}
        activeAccountId={null}
        surface={surface}
        onActiveAccountChange={() => {}}
        onNavigate={replaceFocusedSurface}
        onClose={
          canClose
            ? (onClose ?? (() => void closeCurrentSurfaceWindow()))
            : undefined
        }
        showBackToApp={onClose !== undefined || !isTauriRuntime()}
        shell="overlay"
      />
    )
  }

  if (surface.kind === 'attachment') {
    return <AttachmentSurface surface={surface} />
  }

  return (
    <>
      <MessageDetail
        selection={surface.params}
        accounts={accountsQuery.data ?? []}
        sidebar={sidebarQuery.data}
        onArchive={() =>
          actions.archive({
            sourceId: surface.params.sourceId,
            messageId: surface.params.messageId,
          })
        }
        onForward={forwardSelectedMessage}
        onReply={replyToSelectedMessage}
        onSearch={onSearch}
        onSelectMessage={onSelectMessage ?? (() => {})}
      />
      {composeIntent && (
        <ComposeOverlay intent={composeIntent} onClose={closeCompose} />
      )}
    </>
  )
}

export function FocusedSurfaceDocument({
  surface,
}: {
  surface: SurfaceDescriptor
}) {
  useEffect(() => {
    if (!isTauriRuntime()) {
      return
    }

    function handleKeyDown(event: KeyboardEvent) {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'w') {
        event.preventDefault()
        void closeCurrentSurfaceWindow()
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [])

  return (
    <main className="h-full min-h-0 bg-background text-foreground">
      <FocusedSurface surface={surface} />
    </main>
  )
}
