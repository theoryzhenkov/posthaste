import { useEffect, useState } from 'react'

import {
  surfaceRouteStateFromLocation,
  type SurfaceDescriptor,
  type SurfaceRouteState,
} from '@/domain/surface'

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
