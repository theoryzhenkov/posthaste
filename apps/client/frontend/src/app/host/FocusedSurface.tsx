import { useEffect } from 'react'

import type { MessageSummary } from '@/data/transport/api'
import type { SurfaceDescriptor } from '@/surfaces'
import { useAccounts } from '@/data/queries/queries'
import {
  closeCurrentSurfaceWindow,
  isTauriRuntime,
  listenForDesktopCloseRequest,
} from '@/desktop/runtime'
import { useComposeIntent } from '@/data/hooks/useComposeIntent'
import { useEmailActions } from '@/data/hooks/useEmailActions'
import { useUndoRedo } from '@/data/hooks/useUndoRedo'
import { replaceFocusedSurface } from '@/surfaces/useSurfaceRouting'
import {
  markSurfaceBootstrap,
  markSurfaceBootstrapOnce,
} from '@/surfaces/bootstrapLog'
import { AttachmentSurface } from './AttachmentSurface'
import { ComposeOverlay } from '../../components/compose/ComposeOverlay'
import { MessageDetail } from '../../components/mail/detail/MessageDetail'
import { SettingsPanel } from '../../components/settings/panel/SettingsPanel'
import { WindowTitlebar } from '../../components/ui/WindowChrome'

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
  markSurfaceBootstrapOnce('focused_surface_render', { kind: surface.kind })
  const selectedMessage = surface.kind === 'message' ? surface.params : null
  const accountsQuery = useAccounts({ enabled: surface.kind === 'settings' })
  const undoRedo = useUndoRedo()
  const actions = useEmailActions({ undo: undoRedo.undo })
  const {
    closeCompose,
    composeIntent,
    composeMailto,
    editDraft,
    forwardSelectedMessage,
    replyAllToSelectedMessage,
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
        accounts={accountsQuery.data?.rows ?? []}
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
        actions={actions}
        // A focused window has no view context — role-gated header actions
        // resolve as they do for an ambiguous view.
        viewRole={null}
        onEditDraft={() =>
          editDraft(surface.params.sourceId, surface.params.messageId)
        }
        onForward={forwardSelectedMessage}
        onReply={replyToSelectedMessage}
        onReplyAll={replyAllToSelectedMessage}
        onSearch={onSearch}
        onSelectMessage={onSelectMessage ?? (() => {})}
        onUnsubscribeMailto={(mailtoUri) =>
          composeMailto(surface.params.sourceId, mailtoUri)
        }
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
  markSurfaceBootstrapOnce('focused_document_render', { kind: surface.kind })
  useEffect(() => {
    markSurfaceBootstrap('focused_document_mounted')
    if (!isTauriRuntime()) {
      return
    }

    let unlisten: (() => void) | null = null
    let disposed = false
    markSurfaceBootstrap('close_listener_start')
    void listenForDesktopCloseRequest(() => {
      void closeCurrentSurfaceWindow()
    }).then((nextUnlisten) => {
      markSurfaceBootstrap('close_listener_done')
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
