import { useEffect } from 'react'
import type { AccountOverview } from '../api/types'
import { SettingsPanel } from './SettingsPanel'

interface SettingsOverlayProps {
  accounts: AccountOverview[]
  activeAccountId: string | null
  initialCategory?: 'general' | 'accounts' | 'mailboxes'
  onActiveAccountChange: (accountId: string | null) => void
  onClose: () => void
}

export function SettingsOverlay({
  accounts,
  activeAccountId,
  initialCategory,
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
        initialCategory={initialCategory}
        onActiveAccountChange={onActiveAccountChange}
        onClose={onClose}
        shell="overlay"
      />
    </div>
  )
}
