/**
 * Web-history surface navigation: push/replace/close a surface on THIS
 * document's history stack. The desktop counterpart (a real OS window per
 * surface) lives in `desktop/runtime.ts`; `app/host/navigation.ts` is the
 * composition point that picks between the two.
 */
import { surfaceRoute, type SurfaceDescriptor } from '../domain/surface/index'
import {
  currentSurfaceDepth,
  isSurfaceHistoryState,
  rootUrl,
  surfaceHistoryState,
  surfaceUrl,
} from './history'

export function openWebSurface(surface: SurfaceDescriptor): void {
  const route = surfaceRoute(surface)
  const depth = currentSurfaceDepth(window.location, window.history.state) + 1
  window.history.pushState(
    surfaceHistoryState(route, depth),
    '',
    surfaceUrl(window.location, route),
  )
  window.dispatchEvent(new HashChangeEvent('hashchange'))
}

export function replaceWebSurface(surface: SurfaceDescriptor): void {
  const route = surfaceRoute(surface)
  const depth = Math.max(
    1,
    currentSurfaceDepth(window.location, window.history.state),
  )
  window.history.replaceState(
    surfaceHistoryState(route, depth),
    '',
    surfaceUrl(window.location, route),
  )
  window.dispatchEvent(new HashChangeEvent('hashchange'))
}

export function closeWebSurface(): void {
  if (isSurfaceHistoryState(window.history.state)) {
    window.history.back()
    return
  }

  window.history.pushState(null, '', rootUrl(window.location))
  window.dispatchEvent(new HashChangeEvent('hashchange'))
}
