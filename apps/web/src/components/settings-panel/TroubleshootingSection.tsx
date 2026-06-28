/**
 * Troubleshooting controls: a manual "repair local database" pathway.
 *
 * The local database is a rebuildable cache (accounts live in config, secrets in
 * the keychain), so repairing quarantines a possibly-corrupt database and
 * rebuilds it on restart. Desktop-only.
 */
import { useState } from 'react'
import { Loader2 } from 'lucide-react'

import {
  canFactoryReset,
  factoryResetAndRestart,
  repairLocalDatabaseAndRestart,
} from '@/desktopRepair'
import { LOG_EVENTS } from '@/logEvents'
import { syncLogger } from '@/logger'

import { Button } from '../ui/button'
import { SettingsSection } from './shared'

export function TroubleshootingSection() {
  const [isRepairing, setIsRepairing] = useState(false)
  const [isResetting, setIsResetting] = useState(false)

  async function handleFactoryReset() {
    const confirmed = window.confirm(
      'Reset all local data removes ALL accounts, settings, and cached mail from this device and restarts Posthaste with a clean install. Your mail on the server is not affected, but you will need to add your accounts again. This cannot be undone. Continue?',
    )
    if (!confirmed) {
      return
    }
    setIsResetting(true)
    try {
      await factoryResetAndRestart()
    } catch (error) {
      setIsResetting(false)
      syncLogger.warn(
        { event: LOG_EVENTS.databaseRepairFailed, error },
        'factory reset failed',
      )
      window.alert('Could not reset. Please try again.')
    }
  }

  async function handleRepair() {
    const confirmed = window.confirm(
      'Repair resets Posthaste\u2019s local data \u2014 the cached mail and the view state \u2014 and restarts. Your accounts and passwords are not affected and mail re-syncs from your providers, but any unsent changes will be discarded. Continue?',
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
            Resets the local view cache and rebuilds the mail database, then
            restarts. Use this if mail looks corrupted, fails to load, or views
            stay stuck loading. Accounts and passwords are kept; any unsent
            changes are discarded.
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
      {canFactoryReset() ? (
        <div className="mt-4 grid gap-3 border-t border-border/60 pt-4 sm:grid-cols-[1fr_280px] sm:items-center">
          <div className="min-w-0">
            <p className="text-[13px] font-medium text-foreground">
              Reset all local data
            </p>
            <p className="mt-1 text-[12px] leading-5 text-muted-foreground">
              Removes every account, setting, and cached message from this
              device and restarts with a clean install. Your mail on the server
              is not affected; you will need to add your accounts again.
            </p>
          </div>
          <Button
            type="button"
            variant="outline"
            disabled={isResetting}
            onClick={() => void handleFactoryReset()}
            className="h-8 gap-2 border-destructive/40 bg-background text-[13px] text-destructive shadow-none hover:bg-destructive/10 sm:justify-self-end"
          >
            {isResetting ? (
              <Loader2 size={14} className="animate-spin" />
            ) : null}
            Reset all local data
          </Button>
        </div>
      ) : null}
    </SettingsSection>
  )
}
