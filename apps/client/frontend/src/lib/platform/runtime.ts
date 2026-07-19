/**
 * Ambient runtime probes (R8): which host is rendering this document. Pure
 * reads of window globals — no Tauri imports — so every home may consult them
 * without crossing into desktop glue.
 */

export function isTauriRuntime(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
}

function currentDesktopWindowLabel(): string | null {
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
