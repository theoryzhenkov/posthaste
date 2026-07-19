/**
 * Desktop auto-update helpers shared by the on-launch check and the manual
 * "Check for updates" control in settings.
 *
 * Updates are served from the GitHub Releases `latest.json` manifest and
 * verified against the bundled public key by the Tauri updater plugin. The
 * plugin modules are imported lazily so the browser-localhost build never pulls
 * them. All entry points are no-ops outside the Tauri runtime.
 */
import { toast } from 'sonner'

import { isTauriRuntime } from '@/lib/platform/runtime'
import { LOG_EVENTS } from '../../lib/log/logEvents'
import { syncLogger } from '../../lib/log/logger'

// Structural type for the updater plugin's `Update`, avoiding a static import.
type DesktopUpdate = NonNullable<
  Awaited<ReturnType<typeof import('@tauri-apps/plugin-updater').check>>
>

/** Check for an update. Returns the update, or `null` when up to date. */
export async function checkForDesktopUpdate(): Promise<DesktopUpdate | null> {
  if (!isTauriRuntime()) {
    return null
  }
  const { check } = await import('@tauri-apps/plugin-updater')
  return check()
}

/** Show the "update available" toast with a one-click install action. */
export function promptDesktopUpdate(update: DesktopUpdate): void {
  toast(`Update available: ${update.version}`, {
    description: 'A newer version of Posthaste is ready to install.',
    duration: Infinity,
    action: {
      label: 'Install & restart',
      onClick: () => void installDesktopUpdate(update),
    },
  })
}

/** Download, install, and relaunch into the new version. */
async function installDesktopUpdate(
  update: DesktopUpdate,
): Promise<void> {
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
