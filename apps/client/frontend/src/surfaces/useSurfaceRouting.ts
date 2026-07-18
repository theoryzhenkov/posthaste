import { useEffect, useState } from 'react'
import { toast } from 'sonner'

import {
  isTauriRuntime,
  openDesktopSurface,
  openWebSurface,
  replaceWebSurface,
} from '@/desktop/runtime'
import {
  surfaceRouteStateFromLocation,
  type SurfaceDescriptor,
  type SurfaceRouteState,
} from '@/surfaces'

export { closeWebSurface } from '@/desktop/runtime'

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
}: {
  routeSurface: SurfaceDescriptor | null
}) {
  // The renderer never force-opens Settings: account state is served, and the
  // empty-account first run shows its own in-pane affordance rather than
  // hijacking the surface (which churned open on background refetches).
  return {
    effectiveSurface: routeSurface,
    isSettingsSurfaceOpen: routeSurface?.kind === 'settings',
  }
}
