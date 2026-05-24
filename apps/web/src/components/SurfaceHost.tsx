import { useEffect } from 'react'
import { ExternalLink, X } from 'lucide-react'
import { toast } from 'sonner'

import { openSurfaceInSeparateWindow } from '@/desktop'
import type { SurfaceDescriptor } from '@/surfaces'
import { Button } from './ui/button'
import { FocusedSurface } from './FocusedSurface'

interface SurfaceHostProps {
  surface: SurfaceDescriptor | null
  canClose?: boolean
  onClose: () => void
  onSearch: (query: string, append?: boolean) => void
}

function surfaceTitle(surface: SurfaceDescriptor): string {
  switch (surface.kind) {
    case 'attachment':
      return 'Attachment'
    case 'message':
      return 'Message'
    case 'settings':
      return 'Settings'
  }
}

export function SurfaceHost({
  surface,
  canClose = true,
  onClose,
  onSearch,
}: SurfaceHostProps) {
  useEffect(() => {
    if (!surface) {
      return
    }

    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === 'Escape' && !event.repeat && canClose) {
        event.preventDefault()
        onClose()
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [canClose, onClose, surface])

  if (!surface) {
    return null
  }

  function handleOpenInWindow() {
    void openSurfaceInSeparateWindow(surface)
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

  if (surface.kind === 'settings') {
    return (
      <div className="fixed inset-0 z-[2100] bg-background text-foreground">
        <div className="absolute right-3 top-3 z-10 flex gap-1">
          <Button
            type="button"
            size="icon-sm"
            variant="ghost"
            aria-label="Open settings in separate window"
            title="Open in window"
            onClick={handleOpenInWindow}
          >
            <ExternalLink size={15} strokeWidth={1.7} />
          </Button>
        </div>
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
    <div className="fixed inset-0 z-[2200] flex min-h-0 flex-col bg-background text-foreground">
      <header className="flex h-[42px] shrink-0 items-center gap-3 border-b border-border-soft bg-chrome px-3 text-chrome-foreground">
        <div className="min-w-0 flex-1">
          <p className="truncate text-[13px] font-semibold">
            {surfaceTitle(surface)}
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
