import { listen } from '@tauri-apps/api/event'
import { invoke } from '@tauri-apps/api/core'
import { getCurrentWindow } from '@tauri-apps/api/window'

import type { SurfaceDescriptor } from './surfaces'
import { surfaceRoute } from './surfaces'
import { surfaceWindowUrl } from './surfaceWindow'
import {
  currentSurfaceDepth,
  isSurfaceHistoryState,
  rootUrl,
  surfaceHistoryState,
  surfaceUrl,
} from './surfaceHistory'

export const CLOSE_WINDOW_REQUESTED_EVENT = 'posthaste://close-window-requested'

export function isTauriRuntime(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
}

export async function openDesktopSurface(
  surface: SurfaceDescriptor,
): Promise<void> {
  await invoke('open_surface_window', { surface })
}

export async function openSurfaceInSeparateWindow(
  surface: SurfaceDescriptor,
): Promise<void> {
  if (isTauriRuntime()) {
    await openDesktopSurface(surface)
    return
  }

  const openedWindow = window.open(
    surfaceWindowUrl(window.location, surface),
    '_blank',
    'popup,width=1100,height=820,resizable=yes,scrollbars=yes',
  )
  if (!openedWindow) {
    throw new Error('Popup blocked. Allow popups for Posthaste and try again.')
  }
}

export async function listenForDesktopCloseRequest(
  handler: () => void,
): Promise<() => void> {
  if (!isTauriRuntime()) {
    return () => {}
  }
  return listen(CLOSE_WINDOW_REQUESTED_EVENT, handler)
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
