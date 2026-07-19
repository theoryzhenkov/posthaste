/**
 * Troubleshooting controls: a manual "repair local database" pathway.
 *
 * The local database is a rebuildable cache (accounts live in config, secrets in
 * the keychain), so repairing quarantines a possibly-corrupt database and
 * rebuilds it on restart. Desktop-only.
 */
import { useState } from 'react'
import { Loader2 } from 'lucide-react'
import { toast } from 'sonner'

import { usePlatformServices } from '@/lib/platform/services'
import { LOG_EVENTS } from '@/lib/log/logEvents'
import { syncLogger } from '@/lib/log/logger'

import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from '../../../ui/overlay/alert-dialog'
import { Button } from '../../../ui/form/button'
import { SettingsSection } from '../../panel/shared'

export function TroubleshootingSection() {
  const { repair } = usePlatformServices()
  const [isRepairing, setIsRepairing] = useState(false)
  const [isResetting, setIsResetting] = useState(false)

  async function runRepair() {
    setIsRepairing(true)
    try {
      await repair.repairLocalDatabaseAndRestart()
    } catch (error) {
      setIsRepairing(false)
      syncLogger.warn(
        { event: LOG_EVENTS.databaseRepairFailed, error },
        'manual database repair failed',
      )
      toast.error('Could not start repair. Please try again.')
    }
  }

  async function runFactoryReset() {
    setIsResetting(true)
    try {
      await repair.factoryResetAndRestart()
    } catch (error) {
      setIsResetting(false)
      syncLogger.warn(
        { event: LOG_EVENTS.databaseRepairFailed, error },
        'factory reset failed',
      )
      toast.error('Could not reset. Please try again.')
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
        <AlertDialog>
          <AlertDialogTrigger asChild>
            <Button
              type="button"
              variant="outline"
              disabled={isRepairing}
              className="h-8 gap-2 border-border bg-background text-[13px] shadow-none sm:justify-self-end"
            >
              {isRepairing ? (
                <Loader2 size={14} className="animate-spin" />
              ) : null}
              Repair &amp; restart
            </Button>
          </AlertDialogTrigger>
          <AlertDialogContent>
            <AlertDialogHeader>
              <AlertDialogTitle>Repair local data?</AlertDialogTitle>
              <AlertDialogDescription>
                This resets Posthaste&rsquo;s local data — the cached mail and
                the view state — and restarts. Your accounts and passwords are
                not affected and mail re-syncs from your providers, but any
                unsent changes will be discarded.
              </AlertDialogDescription>
            </AlertDialogHeader>
            <AlertDialogFooter>
              <AlertDialogCancel>Cancel</AlertDialogCancel>
              <AlertDialogAction onClick={() => void runRepair()}>
                Repair &amp; restart
              </AlertDialogAction>
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialog>
      </div>
      {repair.canFactoryReset() ? (
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
          <AlertDialog>
            <AlertDialogTrigger asChild>
              <Button
                type="button"
                variant="outline"
                disabled={isResetting}
                className="h-8 gap-2 border-destructive/40 bg-background text-[13px] text-destructive shadow-none hover:bg-destructive/10 sm:justify-self-end"
              >
                {isResetting ? (
                  <Loader2 size={14} className="animate-spin" />
                ) : null}
                Reset all local data
              </Button>
            </AlertDialogTrigger>
            <AlertDialogContent>
              <AlertDialogHeader>
                <AlertDialogTitle>Reset all local data?</AlertDialogTitle>
                <AlertDialogDescription>
                  This removes ALL accounts, settings, and cached mail from this
                  device and restarts Posthaste with a clean install. Your mail
                  on the server is not affected, but you will need to add your
                  accounts again. This cannot be undone.
                </AlertDialogDescription>
              </AlertDialogHeader>
              <AlertDialogFooter>
                <AlertDialogCancel>Cancel</AlertDialogCancel>
                <AlertDialogAction
                  variant="destructive"
                  onClick={() => void runFactoryReset()}
                >
                  Reset all local data
                </AlertDialogAction>
              </AlertDialogFooter>
            </AlertDialogContent>
          </AlertDialog>
        </div>
      ) : null}
    </SettingsSection>
  )
}
