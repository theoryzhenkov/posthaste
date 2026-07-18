import { parseSurfaceRoute } from './parse'
import type {
  SurfaceDescriptor,
  SurfaceLocation,
  SurfaceRouteState,
} from './types'

export function surfaceFromLocation(
  location: SurfaceLocation,
): SurfaceDescriptor | null {
  const state = surfaceRouteStateFromLocation(location)
  return state.kind === 'valid' ? state.surface : null
}

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
