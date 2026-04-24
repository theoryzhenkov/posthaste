import { useEffect } from 'react'
import type { AccountOverview } from '../api/types'
import { SettingsPanel } from './SettingsPanel'

interface SettingsOverlayProps {
  accounts: AccountOverview[]
  activeAccountId: string | null
  initialAccountId?: string | null
  initialCategory?: 'general' | 'appearance' | 'accounts' | 'mailboxes'
  initialSmartMailboxId?: string | null
  onActiveAccountChange: (accountId: string | null) => void
  onClose: () => void
}

export function SettingsOverlay({
  accounts,
  activeAccountId,
  initialAccountId,
  initialCategory,
  initialSmartMailboxId,
  onActiveAccountChange,
  onClose,
}: SettingsOverlayProps) {
  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === 'Escape') {
        onClose()
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [onClose])

  return (
    <div className="fixed inset-0 z-[2100] bg-background text-foreground">
      <SettingsPanel
        accounts={accounts}
        activeAccountId={activeAccountId}
        initialAccountId={initialAccountId}
        initialCategory={initialCategory}
        initialSmartMailboxId={initialSmartMailboxId}
        onActiveAccountChange={onActiveAccountChange}
        onClose={onClose}
        shell="overlay"
      />
    </div>
  )
}
