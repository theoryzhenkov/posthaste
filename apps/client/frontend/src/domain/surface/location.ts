import { parseSurfaceRoute } from './parse'
import type { SurfaceLocation, SurfaceRouteState } from './types'

export function surfaceRouteStateFromLocation(
  location: SurfaceLocation,
): SurfaceRouteState {
  const hashRoute = location.hash.startsWith('#') ? location.hash.slice(1) : ''
  const route =
    hashRoute.length > 0 ? hashRoute : `${location.pathname}${location.search}`
  if (!isSurfaceRoutePath(route)) {
    return { kind: 'none' }
  }

  const surface = parseSurfaceRoute(route)
  return surface
    ? { kind: 'valid', route, surface }
    : { kind: 'invalid', route }
}

function isSurfaceRoutePath(route: string): boolean {
  try {
    const url = new URL(route, 'http://posthaste.local')
    const parts = url.pathname.split('/').filter(Boolean)
    return parts[0] === 'surface'
  } catch {
    return false
  }
}

/** True when the given location addresses a standalone surface document
 *  (`/surface/...` as hash route or pathname). */
export function isSurfaceLocation(location: SurfaceLocation): boolean {
  const hashRoute = location.hash.startsWith('#')
    ? location.hash.slice(1)
    : location.hash
  return (
    hashRoute.startsWith('/surface/') ||
    location.pathname.startsWith('/surface/')
  )
}
