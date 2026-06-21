/**
 * Checks for desktop app updates on startup and offers a one-click install.
 *
 * Only runs inside the main Tauri desktop window; the browser-localhost build
 * and secondary surface windows are no-ops. Updates are served from the GitHub
 * Releases `latest.json` manifest and verified against the bundled public key
 * by the Tauri updater plugin. The updater plugin modules are imported lazily so
 * the browser build never pulls them.
 */
import { useEffect, useRef } from 'react'
import { toast } from 'sonner'

import { isMainDesktopWindow, isTauriRuntime } from '@/desktop'
import { LOG_EVENTS } from '@/logEvents'
import { syncLogger } from '@/logger'

export function useDesktopUpdates(): void {
  const checkedRef = useRef(false)

  useEffect(() => {
    if (!isTauriRuntime() || !isMainDesktopWindow() || checkedRef.current) {
      return
    }
    checkedRef.current = true
    void checkForUpdates()
  }, [])
}

async function checkForUpdates(): Promise<void> {
  try {
    const { check } = await import('@tauri-apps/plugin-updater')
    const update = await check()
    if (!update) {
      return
    }
    toast(`Update available: ${update.version}`, {
      description: 'A newer version of Posthaste is ready to install.',
      duration: Infinity,
      action: {
        label: 'Install & restart',
        onClick: () => void installUpdate(update),
      },
    })
  } catch (error) {
    // A failed check (offline, manifest unavailable) must never disrupt the app.
    syncLogger.warn(
      { event: LOG_EVENTS.updateCheckFailed, error },
      'desktop update check failed',
    )
  }
}

async function installUpdate(
  update: Awaited<
    ReturnType<typeof import('@tauri-apps/plugin-updater').check>
  >,
): Promise<void> {
  if (!update) {
    return
  }
  const toastId = toast.loading('Downloading update...')
  try {
    await update.downloadAndInstall()
    const { relaunch } = await import('@tauri-apps/plugin-process')
    toast.dismiss(toastId)
    await relaunch()
  } catch (error) {
    toast.dismiss(toastId)
    toast.error('Update failed to install. Try again later.')
    syncLogger.warn(
      { event: LOG_EVENTS.updateInstallFailed, error },
      'desktop update install failed',
    )
  }
}
