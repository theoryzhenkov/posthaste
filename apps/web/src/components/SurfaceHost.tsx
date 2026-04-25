import { useEffect } from 'react'
import { X } from 'lucide-react'

import type { MessageSummary } from '@/api/types'
import type { SurfaceDescriptor } from '@/surfaces'
import { Button } from './ui/button'
import { MessageDetail } from './MessageDetail'

interface SurfaceHostProps {
  surface: SurfaceDescriptor | null
  onClose: () => void
  onSearch: (query: string, append?: boolean) => void
  onSelectMessage: (message: MessageSummary) => void
}

function surfaceTitle(surface: SurfaceDescriptor): string {
  switch (surface.kind) {
    case 'message':
      return 'Message'
  }
}

export function SurfaceHost({
  surface,
  onClose,
  onSearch,
  onSelectMessage,
}: SurfaceHostProps) {
  useEffect(() => {
    if (!surface) {
      return
    }

    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === 'Escape') {
        event.preventDefault()
        onClose()
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [onClose, surface])

  if (!surface) {
    return null
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
          aria-label="Close focused surface"
          title="Close"
          onClick={onClose}
        >
          <X size={15} strokeWidth={1.7} />
        </Button>
      </header>

      <main className="min-h-0 flex-1">
        {surface.kind === 'message' && (
          <MessageDetail
            selection={surface.params}
            onSearch={onSearch}
            onSelectMessage={onSelectMessage}
          />
        )}
      </main>
    </div>
  )
}
