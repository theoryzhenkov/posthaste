/**
 * Troubleshooting controls, ordered by how much they cost the user.
 *
 * The three actions here look alike and differ wildly in consequence, so the
 * copy's job is to keep them apart: rebuilding message details re-reads mail
 * already on disk and fills blanks; repairing the database throws that mail
 * away and downloads it again; the factory reset throws the accounts away too.
 * Someone looking at a blank Cc row will otherwise reach for whichever sounds
 * most decisive, so the lightest is listed first and the heavy ones say what
 * they destroy. Desktop-only.
 */
import { useState, type ReactNode } from 'react'
import { Loader2 } from 'lucide-react'
import { toast } from 'sonner'

import { useCommands } from '@/data'
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

/** One labelled action: explanation on the left, its control on the right. */
function ActionRow({
  title,
  description,
  className,
  children,
}: {
  title: string
  description: string
  className?: string
  children: ReactNode
}) {
  const divider = className ? ` ${className}` : ''
  return (
    <div
      className={`grid gap-3 sm:grid-cols-[1fr_280px] sm:items-center${divider}`}
    >
      <div className="min-w-0">
        <p className="text-[13px] font-medium text-foreground">{title}</p>
        <p className="mt-1 text-[12px] leading-5 text-muted-foreground">
          {description}
        </p>
      </div>
      {children}
    </div>
  )
}

export function TroubleshootingSection() {
  const { repair } = usePlatformServices()
  const commands = useCommands()
  const [isRebuilding, setIsRebuilding] = useState(false)
  const [isRepairing, setIsRepairing] = useState(false)
  const [isResetting, setIsResetting] = useState(false)

  async function runMetadataRebuild() {
    setIsRebuilding(true)
    try {
      await commands.rederiveMessageMetadata()
      toast.success('Message details rebuilt from mail stored on this device.')
    } catch (error) {
      syncLogger.warn(
        { event: LOG_EVENTS.databaseRederiveMetadataFailed, error },
        'message-detail rebuild failed',
      )
      toast.error('Could not rebuild message details. Please try again.')
    } finally {
      setIsRebuilding(false)
    }
  }

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
      <ActionRow
        title="Rebuild message details"
        description="Fills in details older messages are missing — Cc, Bcc, Reply-To, unsubscribe links — by re-reading mail already stored on this device. Nothing is downloaded, nothing is deleted, and details you can already see are left alone. This runs on its own after an update; use it if a message still shows blanks."
      >
        <Button
          type="button"
          variant="outline"
          disabled={isRebuilding}
          onClick={() => void runMetadataRebuild()}
          className="h-8 gap-2 border-border bg-background text-[13px] shadow-none sm:justify-self-end"
        >
          {isRebuilding ? <Loader2 size={14} className="animate-spin" /> : null}
          Rebuild details
        </Button>
      </ActionRow>
      <ActionRow
        className="mt-4 border-t border-border/60 pt-4"
        title="Repair local database"
        description="The heavy one. Sets the mail database aside and rebuilds it on restart: every message cached on this device is discarded and downloaded again, and local-only state such as snoozes is lost. Accounts and passwords are kept. Use it when mail fails to load at all — not for blank or stale fields, which the rebuild above fixes without downloading anything."
      >
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
              <AlertDialogTitle>Rebuild the mail database?</AlertDialogTitle>
              <AlertDialogDescription>
                Every message cached on this device is discarded and downloaded
                again from your providers, and local-only state such as snoozed
                messages is lost. Your accounts, passwords, and the mail on the
                server are not affected. If details merely look blank or out of
                date, cancel and use &ldquo;Rebuild message details&rdquo;
                instead — it fixes that without downloading anything.
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
      </ActionRow>
      {repair.canFactoryReset() ? (
        <ActionRow
          className="mt-4 border-t border-border/60 pt-4"
          title="Reset all local data"
          description="Removes every account, setting, and cached message from this device and restarts with a clean install. Your mail on the server is not affected; you will need to add your accounts again."
        >
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
        </ActionRow>
      ) : null}
    </SettingsSection>
  )
}
