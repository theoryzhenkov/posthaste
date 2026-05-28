import { useEffect, useState } from 'react'
import { toast } from 'sonner'

import {
  isTauriRuntime,
  openDesktopSurface,
  openWebSurface,
  replaceWebSurface,
} from '@/desktop'
import {
  settingsCategorySurface,
  surfaceRouteStateFromLocation,
  type SurfaceDescriptor,
  type SurfaceRouteState,
} from '@/surfaces'

export { closeWebSurface } from '@/desktop'

export function useRouteSurface(): SurfaceDescriptor | null {
  const state = useSurfaceRouteState()
  return state.kind === 'valid' ? state.surface : null
}

export function useSurfaceRouteState(): SurfaceRouteState {
  const [state, setState] = useState<SurfaceRouteState>(() =>
    surfaceRouteStateFromLocation(window.location),
  )

  useEffect(() => {
    function syncSurface() {
      setState(surfaceRouteStateFromLocation(window.location))
    }

    window.addEventListener('hashchange', syncSurface)
    window.addEventListener('popstate', syncSurface)
    return () => {
      window.removeEventListener('hashchange', syncSurface)
      window.removeEventListener('popstate', syncSurface)
    }
  }, [])

  return state
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

export function replaceFocusedSurface(surface: SurfaceDescriptor): void {
  replaceWebSurface(surface)
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
      ? settingsCategorySurface('accounts')
      : routeSurface

  useEffect(() => {
    if (shouldForceSettings && isTauriRuntime()) {
      openFocusedSurface(settingsCategorySurface('accounts'))
    }
  }, [shouldForceSettings])

  return {
    effectiveSurface,
    isSettingsSurfaceOpen: effectiveSurface?.kind === 'settings',
    shouldRenderForcedSettings,
  }
}
