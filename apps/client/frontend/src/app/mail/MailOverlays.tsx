import { lazy, Suspense } from 'react'
import { Loader2 } from 'lucide-react'

import { CommandPalette } from '@/components/palette/CommandPalette'
import { ErrorBoundary } from '@/app/shell/ErrorBoundary'
import { InvalidSurface } from '@/app/host/InvalidSurface'
import { ShortcutReference } from '@/components/keyboard/reference/ShortcutReference'
import { SurfaceHost } from '@/app/host/SurfaceHost'
import { TagEditor } from '@/components/mail/tags/TagEditor'
import { closeWebSurface } from '@/surfaces/useSurfaceRouting'

import type { MailClientViewProps } from './MailClientView.types'

const ComposeOverlay = lazy(() =>
  import('@/components/compose/ComposeOverlay').then((module) => ({
    default: module.ComposeOverlay,
  })),
)

export function MailOverlays(props: MailClientViewProps) {
  return (
    <>
      {props.isCommandPaletteOpen && <CommandPaletteOverlay {...props} />}
      {props.showShortcuts && (
        <ShortcutReference onClose={() => props.onToggleShortcuts()} />
      )}
      {props.isTagEditorOpen && props.selectedMessageData && (
        <TagEditor
          actions={props.actions}
          knownTags={props.tags}
          message={props.selectedMessageData}
          onClose={() => props.onSetTagEditorOpen(false)}
          onManageTags={() => {
            props.onSetTagEditorOpen(false)
            props.onOpenSettings('tags')
          }}
        />
      )}
      {props.composeIntent && (
        <Suspense fallback={<ComposeFallback />}>
          <ComposeOverlay
            intent={props.composeIntent}
            onClose={props.closeCompose}
            onRestoreDraft={props.handlers.editDraft}
          />
        </Suspense>
      )}
      {props.invalidSurfaceRoute && (
        <div className="fixed inset-0 z-(--z-surface) bg-background text-foreground">
          <InvalidSurface
            route={props.invalidSurfaceRoute}
            onClose={closeWebSurface}
          />
        </div>
      )}
      {props.effectiveSurface && !props.invalidSurfaceRoute && (
        <ErrorBoundary label="surface" resetKeys={[props.effectiveSurface]}>
          <SurfaceHost
            surface={props.effectiveSurface}
            canClose
            onClose={closeWebSurface}
            onSearch={props.onSearch}
          />
        </ErrorBoundary>
      )}
    </>
  )
}

function CommandPaletteOverlay(props: MailClientViewProps) {
  return (
    <Suspense fallback={null}>
      <CommandPalette
        actions={props.actions}
        app={props.handlers}
        viewRole={props.viewRole}
        initialActionId={props.commandPaletteSeedActionId}
        selectedMessage={props.selectedMessage}
        selectedMessageData={props.selectedMessageData}
        onApplySearch={props.onApplySearch}
        onClose={props.onCloseCommandPalette}
        onSelectMessage={props.onSelectMessageRef}
        onSelectSmartMailbox={props.onSelectSmartMailbox}
        onSelectSourceMailbox={props.onSelectSourceMailbox}
      />
    </Suspense>
  )
}

function ComposeFallback() {
  return (
    <div className="fixed inset-0 z-(--z-window) flex items-center justify-center">
      <Loader2 size={18} className="animate-spin text-muted-foreground" />
    </div>
  )
}
