const SURFACE_HISTORY_KIND = 'posthaste.surface'

export interface SurfaceHistoryLocation {
  hash: string
  pathname: string
  search: string
}

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

export function isSurfaceLocation(location: SurfaceHistoryLocation): boolean {
  const hashRoute = location.hash.startsWith('#')
    ? location.hash.slice(1)
    : location.hash
  return (
    hashRoute.startsWith('/surface/') ||
    location.pathname.startsWith('/surface/')
  )
}

export function currentSurfaceDepth(
  location: SurfaceHistoryLocation,
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
  location: SurfaceHistoryLocation,
  route: string,
): string {
  return `${location.pathname}${location.search}#${route}`
}

export function rootUrl(location: SurfaceHistoryLocation): string {
  return `${location.pathname}${location.search}`
}
