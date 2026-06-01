import { useEffect } from 'react'
import { useQuery } from '@tanstack/react-query'

import { fetchAccounts } from '@/api/client'
import type { MessageSummary } from '@/api/types'
import type { SurfaceDescriptor } from '@/surfaces'
import { queryKeys } from '@/queryKeys'
import {
  closeCurrentSurfaceWindow,
  isTauriRuntime,
  listenForDesktopCloseRequest,
} from '@/desktop'
import { useComposeIntent } from '@/hooks/useComposeIntent'
import { useEmailActions } from '@/hooks/useEmailActions'
import { replaceFocusedSurface } from '@/hooks/useSurfaceRouting'
import { AttachmentSurface } from './AttachmentSurface'
import { ComposeOverlay } from './ComposeOverlay'
import { MessageDetail } from './MessageDetail'
import { SettingsPanel } from './SettingsPanel'
import { WindowTitlebar } from './WindowChrome'

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
    enabled: surface.kind === 'settings',
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

  if (surface.kind === 'compose') {
    return (
      <ComposeOverlay
        intent={surface.params}
        shell="document"
        onClose={onClose ?? (() => void closeCurrentSurfaceWindow())}
      />
    )
  }

  return (
    <>
      <MessageDetail
        selection={surface.params}
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

    let unlisten: (() => void) | null = null
    let disposed = false
    void listenForDesktopCloseRequest(() => {
      void closeCurrentSurfaceWindow()
    }).then((nextUnlisten) => {
      if (disposed) {
        nextUnlisten()
        return
      }
      unlisten = nextUnlisten
    })

    // Cmd/Ctrl+W is handled by the native window menu (close_window ->
    // performClose:). A JS keydown handler that preventDefault()s the combo
    // makes the WKWebView report the key equivalent as handled, suppressing the
    // menu item, so the window never closes — do not intercept it here.
    return () => {
      disposed = true
      unlisten?.()
    }
  }, [])

  return (
    <main
      className="flex h-full min-h-0 flex-col bg-background text-foreground"
      data-posthaste-state={`state.surface.${surface.kind}.ready.test`}
      data-posthaste-surface-kind={surface.kind}
    >
      <WindowTitlebar title={surfaceWindowTitle(surface)} />
      <div className="min-h-0 flex-1">
        <FocusedSurface surface={surface} />
      </div>
    </main>
  )
}

function surfaceWindowTitle(surface: SurfaceDescriptor): string {
  switch (surface.kind) {
    case 'settings':
      return 'Settings'
    case 'compose':
      return 'Compose'
    case 'attachment':
      return 'Attachment'
    case 'message':
      return 'Message'
    default:
      return 'Posthaste'
  }
}
