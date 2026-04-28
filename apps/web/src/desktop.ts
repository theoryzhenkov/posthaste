import { invoke } from '@tauri-apps/api/core'
import { getCurrentWindow } from '@tauri-apps/api/window'

import type { SurfaceDescriptor } from './surfaces'
import { surfaceRoute } from './surfaces'
import {
  currentSurfaceDepth,
  isSurfaceHistoryState,
  rootUrl,
  surfaceHistoryState,
  surfaceUrl,
} from './surfaceHistory'

export function isTauriRuntime(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
}

export async function openDesktopSurface(
  surface: SurfaceDescriptor,
): Promise<void> {
  await invoke('open_surface_window', { surface })
}

export async function closeCurrentSurfaceWindow(): Promise<void> {
  if (isTauriRuntime()) {
    await getCurrentWindow().close()
    return
  }

  window.location.assign('/')
}

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
