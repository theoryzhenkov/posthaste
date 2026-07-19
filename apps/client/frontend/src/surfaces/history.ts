import { isSurfaceLocation } from '../domain/surface/location'
import type { SurfaceLocation } from '../domain/surface/types'

const SURFACE_HISTORY_KIND = 'posthaste.surface'

export interface SurfaceHistoryState {
  kind: typeof SURFACE_HISTORY_KIND
  depth: number
  route: string
}

export function isSurfaceHistoryState(
  state: unknown,
): state is SurfaceHistoryState {
  return (
    typeof state === 'object' &&
    state !== null &&
    (state as Partial<SurfaceHistoryState>).kind === SURFACE_HISTORY_KIND &&
    typeof (state as Partial<SurfaceHistoryState>).depth === 'number' &&
    typeof (state as Partial<SurfaceHistoryState>).route === 'string'
  )
}

export function currentSurfaceDepth(
  location: SurfaceLocation,
  state: unknown,
): number {
  if (isSurfaceHistoryState(state)) {
    return state.depth
  }
  return isSurfaceLocation(location) ? 1 : 0
}

export function surfaceHistoryState(
  route: string,
  depth: number,
): SurfaceHistoryState {
  return {
    kind: SURFACE_HISTORY_KIND,
    depth,
    route,
  }
}

export function surfaceUrl(
  location: SurfaceLocation,
  route: string,
): string {
  return `${location.pathname}${location.search}#${route}`
}

export function rootUrl(location: SurfaceLocation): string {
  return `${location.pathname}${location.search}`
}
