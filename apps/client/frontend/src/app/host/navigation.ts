/**
 * Surface navigation, composed: pick the desktop runtime (real OS windows) or
 * the web history stack per call. Only `app/` may join `desktop/` and
 * `surfaces/` (R11); components reach these verbs through
 * `lib/platform/services.ts`.
 */
import { toast } from 'sonner'

import type { SurfaceDescriptor } from '@/domain/surface/types'
import { openDesktopSurface } from '@/desktop/runtime'
import { isTauriRuntime } from '@/lib/platform/runtime'
import { openWebSurface, replaceWebSurface } from '@/surfaces/navigation'
import { surfacePopupFeatures, surfaceWindowUrl } from '@/surfaces/window'

export function openFocusedSurface(surface: SurfaceDescriptor): void {
  if (isTauriRuntime()) {
    void openDesktopSurface(surface).catch((error: unknown) => {
      toast.error(
        error instanceof Error ? error.message : 'Failed to open window',
      )
    })
    return
  }

  openWebSurface(surface)
}

export function replaceFocusedSurface(surface: SurfaceDescriptor): void {
  replaceWebSurface(surface)
}

export async function openSurfaceInSeparateWindow(
  surface: SurfaceDescriptor,
): Promise<void> {
  if (isTauriRuntime()) {
    await openDesktopSurface(surface)
    return
  }

  const openedWindow = window.open(
    surfaceWindowUrl(window.location, surface),
    '_blank',
    surfacePopupFeatures(surface),
  )
  if (!openedWindow) {
    throw new Error('Popup blocked. Allow popups for Posthaste and try again.')
  }
}
