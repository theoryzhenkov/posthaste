import { invoke } from '@tauri-apps/api/core'
import { getCurrentWindow } from '@tauri-apps/api/window'

import type { SurfaceDescriptor } from './surfaces'
import { surfaceRoute } from './surfaces'

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
  window.location.hash = surfaceRoute(surface)
}

export function closeWebSurface(): void {
  window.history.pushState(
    null,
    '',
    `${window.location.pathname}${window.location.search}`,
  )
  window.dispatchEvent(new HashChangeEvent('hashchange'))
}
