/**
 * Troubleshooting controls: a manual "repair local database" pathway.
 *
 * The local database is a rebuildable cache (accounts live in config, secrets in
 * the keychain), so repairing quarantines a possibly-corrupt database and
 * rebuilds it on restart. Desktop-only.
 */
import { useState } from 'react'
import { Loader2 } from 'lucide-react'

import { repairLocalDatabaseAndRestart } from '@/desktopRepair'
import { LOG_EVENTS } from '@/logEvents'
import { syncLogger } from '@/logger'

import { Button } from '../ui/button'
import { SettingsSection } from './shared'

export function TroubleshootingSection() {
  const [isRepairing, setIsRepairing] = useState(false)

  async function handleRepair() {
    const confirmed = window.confirm(
      'Repair will rebuild the local mail cache and restart Posthaste. Your accounts and passwords are not affected; mail re-syncs from your providers. Continue?',
    )
    if (!confirmed) {
      return
    }
    setIsRepairing(true)
    try {
      await repairLocalDatabaseAndRestart()
    } catch (error) {
      setIsRepairing(false)
      syncLogger.warn(
        { event: LOG_EVENTS.databaseRepairFailed, error },
        'manual database repair failed',
      )
      window.alert('Could not start repair. Please try again.')
    }
  }

  return (
    <SettingsSection title="Troubleshooting">
      <div className="grid gap-3 sm:grid-cols-[1fr_280px] sm:items-center">
        <div className="min-w-0">
          <p className="text-[13px] font-medium text-foreground">
            Repair local database
          </p>
          <p className="mt-1 text-[12px] leading-5 text-muted-foreground">
            Rebuilds the local mail cache and restarts. Use this if mail looks
            corrupted or fails to load. Accounts and passwords are kept.
          </p>
        </div>
        <Button
          type="button"
          variant="outline"
          disabled={isRepairing}
          onClick={() => void handleRepair()}
          className="h-8 gap-2 border-border bg-background text-[13px] shadow-none sm:justify-self-end"
        >
          {isRepairing ? <Loader2 size={14} className="animate-spin" /> : null}
          Repair & restart
        </Button>
      </div>
    </SettingsSection>
  )
}
