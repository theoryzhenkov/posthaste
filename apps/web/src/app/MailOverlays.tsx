import { lazy, Suspense } from 'react'
import { Loader2 } from 'lucide-react'

import { CommandPalette } from '@/components/CommandPalette'
import { ErrorBoundary } from '@/components/ErrorBoundary'
import { InvalidSurface } from '@/components/InvalidSurface'
import { ShortcutReference } from '@/components/ShortcutReference'
import { SurfaceHost } from '@/components/SurfaceHost'
import { TagEditor } from '@/components/TagEditor'
import { closeWebSurface } from '@/hooks/useSurfaceRouting'

import type { MailClientViewProps } from './MailClientView.types'

const ComposeOverlay = lazy(() =>
  import('@/components/ComposeOverlay').then((module) => ({
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
        />
      )}
      {props.composeIntent && (
        <Suspense fallback={<ComposeFallback />}>
          <ComposeOverlay
            intent={props.composeIntent}
            onClose={props.closeCompose}
          />
        </Suspense>
      )}
      {props.invalidSurfaceRoute && !props.shouldRenderForcedSettings && (
        <div className="fixed inset-0 z-[2300] bg-background text-foreground">
          <InvalidSurface
            route={props.invalidSurfaceRoute}
            onClose={closeWebSurface}
          />
        </div>
      )}
      {props.effectiveSurface &&
        (!props.invalidSurfaceRoute || props.shouldRenderForcedSettings) && (
          <ErrorBoundary label="surface" resetKeys={[props.effectiveSurface]}>
            <SurfaceHost
              surface={props.effectiveSurface}
              canClose={!props.shouldRenderForcedSettings}
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
        hasSelectedMessage={props.selectedMessage !== null}
        onApplySearch={props.onApplySearch}
        onArchive={props.onArchive}
        onClose={props.onCloseCommandPalette}
        onCompose={props.onCompose}
        onOpenSettings={props.onOpenSettings}
        onOpenShortcuts={props.onShowShortcuts}
        onPlaceholderAction={props.onPlaceholderAction}
        onReply={props.onReply}
        onSelectMessage={props.onSelectMessageRef}
        onSelectSmartMailbox={props.onSelectSmartMailbox}
        onSelectSourceMailbox={props.onSelectSourceMailbox}
        onToggleFlag={props.onToggleFlag}
      />
    </Suspense>
  )
}

function ComposeFallback() {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <Loader2 size={18} className="animate-spin text-muted-foreground" />
    </div>
  )
}
