/**
 * Tauri bridge commands for the current window: devtools, external URLs,
 * surface windows, boot ACK, and the guarded close flow. Runtime probes
 * (`isTauriRuntime`, …) live in `lib/platform/runtime.ts`; web-history surface
 * navigation lives in `surfaces/navigation.ts`.
 */
import { listen } from '@tauri-apps/api/event'
import { invoke } from '@tauri-apps/api/core'
import { getCurrentWindow } from '@tauri-apps/api/window'

import type { SurfaceDescriptor } from '../domain/surface/index'
import { isTauriRuntime } from '../lib/platform/runtime'

const CLOSE_WINDOW_REQUESTED_EVENT = 'posthaste://close-window-requested'

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
