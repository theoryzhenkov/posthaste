import { useMemo } from 'react'
import { ExternalLink, X } from 'lucide-react'
import { toast } from 'sonner'

import { openSurfaceInSeparateWindow } from './navigation'
import { useCommandScope, type CommandScope } from '@/lib/command'
import type { SurfaceDescriptor } from '@/domain/surface'
import { surfaceWindowPolicy } from '@/surfaces/window'
import { Button } from '../../components/ui/form/button'
import { FocusedSurface } from './FocusedSurface'

interface SurfaceHostProps {
  surface: SurfaceDescriptor | null
  canClose?: boolean
  onClose: () => void
  onSearch: (query: string, append?: boolean) => void
}

export function SurfaceHost({
  surface,
  canClose = true,
  onClose,
  onSearch,
}: SurfaceHostProps) {
  // Escape routes through the command dispatcher (`surface.close`): the scope
  // binds `surfaceHost.close` only while a closeable surface is shown, so the
  // chord falls through everywhere else.
  const closeScope = useMemo<CommandScope | null>(
    () =>
      surface && canClose
        ? { owner: 'surface', services: { surfaceHost: { close: onClose } } }
        : null,
    [canClose, onClose, surface],
  )
  useCommandScope(closeScope)

  if (!surface) {
    return null
  }

  const focusedSurface = surface

  function handleOpenInWindow() {
    void openSurfaceInSeparateWindow(focusedSurface)
      .then(() => {
        if (canClose) {
          onClose()
        }
      })
      .catch((error: unknown) => {
        toast.error(
          error instanceof Error ? error.message : 'Failed to open window',
        )
      })
  }

  if (surface.kind === 'settings' || surface.kind === 'compose') {
    return (
      <div
        className="fixed inset-0 z-(--z-surface) bg-background text-foreground"
        data-posthaste-state={`state.surface.${surface.kind}.ready.test`}
        data-posthaste-surface-kind={surface.kind}
      >
        {surface.kind === 'settings' && (
          <div className="absolute right-3 top-3 z-10 flex gap-1">
            <Button
              type="button"
              size="icon-sm"
              variant="ghost"
              aria-label={`Open ${surfaceWindowPolicy(surface).title.toLowerCase()} in separate window`}
              title="Open in window"
              onClick={handleOpenInWindow}
            >
              <ExternalLink size={15} strokeWidth={1.7} />
            </Button>
          </div>
        )}
        <FocusedSurface
          surface={surface}
          canClose={canClose}
          onClose={onClose}
          onSearch={onSearch}
        />
      </div>
    )
  }

  return (
    <div
      className="fixed inset-0 z-(--z-surface) flex min-h-0 flex-col bg-background text-foreground"
      data-posthaste-state={`state.surface.${surface.kind}.ready.test`}
      data-posthaste-surface-kind={surface.kind}
    >
      <header className="flex h-[42px] shrink-0 items-center gap-3 border-b border-border-soft bg-chrome px-3 text-chrome-foreground">
        <div className="min-w-0 flex-1">
          <p className="truncate text-[13px] font-semibold">
            {surfaceWindowPolicy(surface).title}
          </p>
        </div>
        <Button
          type="button"
          size="icon-sm"
          variant="ghost"
          aria-label="Open surface in separate window"
          title="Open in window"
          onClick={handleOpenInWindow}
        >
          <ExternalLink size={15} strokeWidth={1.7} />
        </Button>
        <Button
          type="button"
          size="icon-sm"
          variant="ghost"
          aria-label="Close focused surface"
          title="Close"
          disabled={!canClose}
          onClick={onClose}
        >
          <X size={15} strokeWidth={1.7} />
        </Button>
      </header>

      <main className="min-h-0 flex-1">
        <FocusedSurface
          surface={surface}
          canClose={canClose}
          onClose={onClose}
          onSearch={onSearch}
        />
      </main>
    </div>
  )
}
