/**
 * Checks for desktop app updates on startup and offers a one-click install.
 *
 * Only runs inside the main Tauri desktop window; the browser-localhost build
 * and secondary surface windows are no-ops. The manual "Check for updates"
 * control in settings reuses the same helpers in `desktopUpdates`.
 */
import { useEffect, useRef } from 'react'

import { isMainDesktopWindow, isTauriRuntime } from '@/desktop/runtime'
import { checkForDesktopUpdate, promptDesktopUpdate } from '@/desktop/updates/updates'
import { LOG_EVENTS } from '@/lib/log/logEvents'
import { syncLogger } from '@/lib/log/logger'

export function useDesktopUpdates(): void {
  const checkedRef = useRef(false)

  useEffect(() => {
    if (!isTauriRuntime() || !isMainDesktopWindow() || checkedRef.current) {
      return
    }
    checkedRef.current = true
    void checkOnLaunch()
  }, [])
}

async function checkOnLaunch(): Promise<void> {
  try {
    const update = await checkForDesktopUpdate()
    if (update) {
      promptDesktopUpdate(update)
    }
  } catch (error) {
    // A failed check (offline, manifest unavailable) must never disrupt the app.
    syncLogger.warn(
      { event: LOG_EVENTS.updateCheckFailed, error },
      'desktop update check failed',
    )
  }
}
