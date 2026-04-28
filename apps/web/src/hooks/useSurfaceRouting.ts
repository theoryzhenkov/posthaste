import { useEffect, useState } from 'react'
import { toast } from 'sonner'

import { isTauriRuntime, openDesktopSurface, openWebSurface } from '@/desktop'
import {
  settingsSurface,
  surfaceFromLocation,
  type SurfaceDescriptor,
} from '@/surfaces'

export { closeWebSurface } from '@/desktop'

export function useRouteSurface(): SurfaceDescriptor | null {
  const [surface, setSurface] = useState<SurfaceDescriptor | null>(() =>
    surfaceFromLocation(window.location),
  )

  useEffect(() => {
    function syncSurface() {
      setSurface(surfaceFromLocation(window.location))
    }

    window.addEventListener('hashchange', syncSurface)
    window.addEventListener('popstate', syncSurface)
    return () => {
      window.removeEventListener('hashchange', syncSurface)
      window.removeEventListener('popstate', syncSurface)
    }
  }, [])

  return surface
}

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

export function useEffectiveSurface({
  routeSurface,
  shouldForceSettings,
}: {
  routeSurface: SurfaceDescriptor | null
  shouldForceSettings: boolean
}) {
  const shouldRenderForcedSettings = shouldForceSettings && !isTauriRuntime()
  const effectiveSurface =
    shouldRenderForcedSettings && routeSurface?.kind !== 'settings'
      ? settingsSurface({ category: 'accounts' })
      : routeSurface

  useEffect(() => {
    if (shouldForceSettings && isTauriRuntime()) {
      openFocusedSurface(settingsSurface({ category: 'accounts' }))
    }
  }, [shouldForceSettings])

  return {
    effectiveSurface,
    isSettingsSurfaceOpen: effectiveSurface?.kind === 'settings',
    shouldRenderForcedSettings,
  }
}
