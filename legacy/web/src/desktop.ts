import { listen } from '@tauri-apps/api/event'
import { invoke } from '@tauri-apps/api/core'
import { getCurrentWindow } from '@tauri-apps/api/window'

import type { SurfaceDescriptor } from './surfaces'
import { surfaceRoute } from './surfaces'
import { surfaceWindowPolicy } from './surfaceWindowPolicy'
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

export function currentDesktopWindowLabel(): string | null {
  if (typeof window === 'undefined') {
    return null
  }
  const label = (window as unknown as Record<string, unknown>)
    .__POSTHASTE_WINDOW_LABEL__
  return typeof label === 'string' ? label : null
}

export function isMainDesktopWindow(): boolean {
  return currentDesktopWindowLabel() === 'main'
}

// macOS desktop windows use the inset/overlay title bar (traffic lights drawn
// inside the webview); the web shell paints the matching drag region + inset.
// Off macOS (other desktop OSes use native decorations; the browser has none)
// no inset is needed.
export function isMacDesktop(): boolean {
  if (!isTauriRuntime() || typeof navigator === 'undefined') {
    return false
  }
  return /Mac/i.test(navigator.userAgent)
}

// Toggle the current window's devtools. Gated by the "Developer tools" setting
// at the call site; the command is a no-op when devtools are not compiled in.
export async function toggleDevtools(): Promise<void> {
  if (isTauriRuntime()) {
    await invoke('toggle_devtools')
  }
}

export async function openExternalUrl(url: string): Promise<void> {
  if (isTauriRuntime()) {
    await invoke('open_external_url', { url })
    return
  }

  const openedWindow = window.open(url, '_blank', 'noopener,noreferrer')
  if (!openedWindow) {
    throw new Error(
      'Popup blocked. Copy the authorization URL and open it in your browser.',
    )
  }
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
    surfacePopupFeatures(surface),
  )
  if (!openedWindow) {
    throw new Error('Popup blocked. Allow popups for Posthaste and try again.')
  }
}

export function surfacePopupFeatures(surface: SurfaceDescriptor): string {
  const { width, height } = surfaceWindowPolicy(surface).popupSize
  return [
    'popup',
    `width=${width}`,
    `height=${height}`,
    'resizable=yes',
    'scrollbars=yes',
  ].join(',')
}

/**
 * ACK frontend boot to the desktop backend (`surface_webview_booted`).
 *
 * Close-path defense in depth: the backend force-destroys a window whose
 * webview never ACKed when it is asked to close, so a frontend that fails to
 * load can never leave an unclosable window. A booted webview keeps the
 * guarded close flow (e.g. the compose close-guard). Fired from `main.tsx` as
 * soon as the bundle executes; best-effort so a backend without the command
 * can never break boot.
 */
export async function ackDesktopWebviewBoot(): Promise<void> {
  if (!isTauriRuntime()) {
    return
  }
  try {
    await invoke('surface_webview_booted')
  } catch {
    // Best-effort: boot must proceed even if the ACK command is unavailable.
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
