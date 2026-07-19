/**
 * Point the document favicon at the channel-appropriate brand mark.
 *
 * The mark is the same scene (the "P" over hills) but the nightly channel uses
 * the night variant (moon + night sky) so the two installs are distinguishable
 * at a glance — in the browser tab, the PWA, and the desktop webview. The
 * desktop *OS* icon (dock/taskbar) is a separate, build-time concern owned by
 * the Tauri bundle, not this runtime swap.
 *
 * @see docs/eph/DESIGN-L2-release-channels.md
 */
import { isNightly } from '@/lib/platform/releaseChannel'

const NIGHTLY_FAVICON = '/favicon-night.svg'

/** Swap the favicon to the nightly mark on the nightly channel; no-op otherwise
 * (the static `index.html` link already carries the day mark). */
export function applyBrandFavicon(): void {
  if (!isNightly) {
    return
  }
  const link = document.querySelector<HTMLLinkElement>('link[rel="icon"]')
  if (link) {
    link.href = NIGHTLY_FAVICON
    return
  }
  // No static link to repoint (defensive): create one.
  const created = document.createElement('link')
  created.rel = 'icon'
  created.type = 'image/svg+xml'
  created.href = NIGHTLY_FAVICON
  document.head.appendChild(created)
}
