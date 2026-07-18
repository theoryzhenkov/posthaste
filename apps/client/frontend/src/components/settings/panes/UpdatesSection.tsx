/**
 * Manual "Check for updates" control for the desktop app. Complements the
 * automatic on-launch check; shows explicit feedback when already up to date.
 */
import { useState } from 'react'
import { Loader2 } from 'lucide-react'
import { toast } from 'sonner'

import {
  checkForDesktopUpdate,
  promptDesktopUpdate,
} from '../../../desktop/updates/updates'
import { LOG_EVENTS } from '../../../lib/log/logEvents'
import { syncLogger } from '../../../lib/log/logger'
import { Button } from '../../ui/form/button'
import { SettingsSection } from '../panel/shared'

export function UpdatesSection() {
  const [checking, setChecking] = useState(false)

  async function handleCheck() {
    setChecking(true)
    try {
      const update = await checkForDesktopUpdate()
      if (update) {
        promptDesktopUpdate(update)
      } else {
        toast.success('Posthaste is up to date.')
      }
    } catch (error) {
      toast.error('Could not check for updates. Try again later.')
      syncLogger.warn(
        { event: LOG_EVENTS.updateCheckFailed, error },
        'manual update check failed',
      )
    } finally {
      setChecking(false)
    }
  }

  return (
    <SettingsSection title="Updates">
      <div className="grid gap-3 sm:grid-cols-[1fr_280px] sm:items-center">
        <div className="min-w-0">
          <p className="text-[13px] font-medium text-foreground">
            Software updates
          </p>
          <p className="mt-1 text-[12px] leading-5 text-muted-foreground">
            Posthaste checks for updates on launch. You can also check now.
          </p>
        </div>
        <Button
          type="button"
          variant="outline"
          disabled={checking}
          onClick={() => void handleCheck()}
          className="h-8 gap-2 border-border bg-background text-[13px] shadow-none sm:justify-self-end"
        >
          {checking ? <Loader2 size={14} className="animate-spin" /> : null}
          Check for updates
        </Button>
      </div>
    </SettingsSection>
  )
}
