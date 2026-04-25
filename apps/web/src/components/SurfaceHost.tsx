import { useEffect } from 'react'
import { X } from 'lucide-react'

import type { AccountOverview, MessageSummary } from '@/api/types'
import type { SurfaceDescriptor } from '@/surfaces'
import { Button } from './ui/button'
import { MessageDetail } from './MessageDetail'
import { SettingsPanel } from './SettingsPanel'

interface SurfaceHostProps {
  surface: SurfaceDescriptor | null
  accounts: AccountOverview[]
  activeAccountId: string | null
  canClose?: boolean
  onClose: () => void
  onSettingsActiveAccountChange: (accountId: string | null) => void
  onSearch: (query: string, append?: boolean) => void
  onSelectMessage: (message: MessageSummary) => void
}

function surfaceTitle(surface: SurfaceDescriptor): string {
  switch (surface.kind) {
    case 'message':
      return 'Message'
    case 'settings':
      return 'Settings'
  }
}

export function SurfaceHost({
  surface,
  accounts,
  activeAccountId,
  canClose = true,
  onClose,
  onSettingsActiveAccountChange,
  onSearch,
  onSelectMessage,
}: SurfaceHostProps) {
  useEffect(() => {
    if (!surface) {
      return
    }

    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === 'Escape' && canClose) {
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

  if (surface.kind === 'settings') {
    return (
      <div className="fixed inset-0 z-[2100] bg-background text-foreground">
        <SettingsPanel
          accounts={accounts}
          activeAccountId={activeAccountId}
          initialAccountId={surface.params.accountId}
          initialCategory={surface.params.category}
          initialSmartMailboxId={surface.params.smartMailboxId}
          onActiveAccountChange={onSettingsActiveAccountChange}
          onClose={canClose ? onClose : undefined}
          shell="overlay"
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
          aria-label="Close focused surface"
          title="Close"
          disabled={!canClose}
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
