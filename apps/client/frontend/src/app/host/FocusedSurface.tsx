import { useEffect, useMemo } from 'react'

import type { MessageSummary } from '@/data/transport/api'
import type { SurfaceDescriptor } from '@/surfaces'
import { useAccounts } from '@/data/queries/queries'
import {
  closeCurrentSurfaceWindow,
  isTauriRuntime,
  listenForDesktopCloseRequest,
  openExternalUrl,
} from '@/desktop/runtime'
import { useComposeIntent } from '@/data/hooks/useComposeIntent'
import { useEmailActions } from '@/data/hooks/useEmailActions'
import { buildDetailHeaderActions } from '@/commands'
import { useCommandScope, type CommandScope } from '@/lib/command'
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
  const actions = useEmailActions()
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

  // Standalone surface WINDOW (no host-provided onClose): Escape closes the
  // window via the dispatcher's `surface.close`, same command the in-app
  // surface host binds.
  const windowCloseScope = useMemo<CommandScope | null>(
    () =>
      !onClose && canClose
        ? {
            owner: 'surface',
            services: {
              surfaceHost: { close: () => void closeCurrentSurfaceWindow() },
            },
          }
        : null,
    [canClose, onClose],
  )
  useCommandScope(windowCloseScope)

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

  const messageParams = surface.kind === 'message' ? surface.params : null
  return (
    <>
      <MessageDetail
        selection={surface.params}
        // A focused window has no view context — role-gated header actions
        // resolve as they do for an ambiguous view. No tag editor / focused
        // opener here: leaving them unbound hides those actions.
        headerActionsFor={buildDetailHeaderActions({
          email: actions,
          viewRole: null,
          detail: {
            reply: replyToSelectedMessage,
            replyAll: replyAllToSelectedMessage,
            forward: forwardSelectedMessage,
            editDraft: () => {
              if (messageParams) {
                editDraft(messageParams.sourceId, messageParams.messageId)
              }
            },
          },
          unsubscribeMailto: (mailtoUri) => {
            if (messageParams) {
              composeMailto(messageParams.sourceId, mailtoUri)
            }
          },
          openExternalUrl,
        })}
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
