/**
 * OS notification delivery + permission, behind one seam.
 *
 *  - Tauri desktop: `tauri-plugin-notification`'s JS API (the plugin is
 *    initialized in `apps/desktop/src/lib.rs`; the `notification:default`
 *    capability grants notify + permission checks). Desktop CLICK behavior is
 *    the OS default — macOS/Windows activate the app — because the plugin
 *    exposes no desktop click callback (`onAction` is mobile-only); in-app
 *    message selection on click is deferred.
 *  - Browser (web mode): the standards `Notification` API, only when
 *    permission is ALREADY granted; click focuses the tab.
 *  - Anywhere else (no runtime support): silently no-op.
 *
 * Permission is only ever REQUESTED from the NotificationsPane's "New mail"
 * toggle ({@link requestOsNotificationPermission}) — never at boot and never
 * from the delivery path, so the user is never surprise-prompted by an
 * arriving message.
 */
import { isTauriRuntime } from '@/lib/platform/runtime'
export type OsNotificationPermission = 'granted' | 'denied' | 'unavailable'

/** One OS banner, already formatted; `sound` mirrors the pane's Sounds toggle. */
export interface NewMailBanner {
  title: string
  body: string
  sound: boolean
}

/** Fire-and-forget delivery; failures are swallowed (banners are best-effort). */
export function postOsNotification(banner: NewMailBanner): void {
  void deliver(banner).catch(() => {
    // Best-effort: a failed banner must never break event handling.
  })
}

async function deliver(banner: NewMailBanner): Promise<void> {
  if (isTauriRuntime()) {
    const { isPermissionGranted, sendNotification } =
      await import('@tauri-apps/plugin-notification')
    // Check-only on the delivery path: no permission, no banner, no prompt.
    if (!(await isPermissionGranted())) {
      return
    }
    sendNotification({
      title: banner.title,
      body: banner.body,
      ...(banner.sound ? { sound: platformSoundName() } : {}),
    })
    return
  }
  if (
    typeof Notification === 'undefined' ||
    Notification.permission !== 'granted'
  ) {
    return
  }
  const notification = new Notification(banner.title, {
    body: banner.body,
    silent: !banner.sound,
  })
  notification.onclick = () => {
    window.focus()
    notification.close()
  }
}

/** The plugin takes a PLATFORM-NATIVE sound name, not an abstract "default". */
function platformSoundName(): string {
  const userAgent = typeof navigator !== 'undefined' ? navigator.userAgent : ''
  if (/Mac/i.test(userAgent)) {
    return 'Ping' // macOS system sound name
  }
  if (/Windows/i.test(userAgent)) {
    return 'Default' // tauri-winrt-notification toast audio name
  }
  return 'message-new-instant' // XDG sound-theme name (Linux)
}

/**
 * Request OS-level notification permission (lazy, from the pane's first
 * enable). Returns the resulting state so the pane can surface a denial.
 */
export async function requestOsNotificationPermission(): Promise<OsNotificationPermission> {
  try {
    if (isTauriRuntime()) {
      const { isPermissionGranted, requestPermission } =
        await import('@tauri-apps/plugin-notification')
      if (await isPermissionGranted()) {
        return 'granted'
      }
      return (await requestPermission()) === 'granted' ? 'granted' : 'denied'
    }
    if (typeof Notification === 'undefined') {
      return 'unavailable'
    }
    if (Notification.permission === 'granted') {
      return 'granted'
    }
    if (Notification.permission === 'denied') {
      return 'denied'
    }
    return (await Notification.requestPermission()) === 'granted'
      ? 'granted'
      : 'denied'
  } catch {
    return 'unavailable'
  }
}
