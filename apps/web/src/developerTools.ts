import { useSyncExternalStore } from 'react'

// Client-local "Developer tools" flip. Devtools are compiled into the single
// desktop build; this flag is the runtime gate — when on, Cmd/Ctrl+Alt+I toggles
// the webview devtools (via the `toggle_devtools` command). Stored in
// localStorage so it is shared across the app's windows (same origin) and
// survives restarts, like the appearance preferences.
const STORAGE_KEY = 'posthaste.developerTools.v1'
const listeners = new Set<() => void>()

function read(): boolean {
  if (typeof window === 'undefined') {
    return false
  }
  try {
    return window.localStorage.getItem(STORAGE_KEY) === 'true'
  } catch {
    return false
  }
}

export function isDeveloperToolsEnabled(): boolean {
  return read()
}

export function setDeveloperToolsEnabled(enabled: boolean): void {
  try {
    window.localStorage.setItem(STORAGE_KEY, String(enabled))
  } catch {
    // A preference; failing to persist should not break anything.
  }
  for (const listener of listeners) {
    listener()
  }
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener)
  function onStorage(event: StorageEvent) {
    if (event.key === STORAGE_KEY) {
      listener()
    }
  }
  window.addEventListener('storage', onStorage)
  return () => {
    listeners.delete(listener)
    window.removeEventListener('storage', onStorage)
  }
}

/** Reactive read of the "Developer tools" setting. */
export function useDeveloperToolsEnabled(): boolean {
  return useSyncExternalStore(subscribe, read, () => false)
}
