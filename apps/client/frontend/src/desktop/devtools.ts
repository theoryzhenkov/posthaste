// Client-local "Developer tools" flip. Devtools are compiled into the single
// desktop build; this flag is the runtime gate — when on, Cmd/Ctrl+Alt+I toggles
// the webview devtools (via the `toggle_devtools` command). A synced
// `createStoredStore` (R5): localStorage-backed so it survives restarts, and
// mirrored across the app's windows (same origin) via storage events, like the
// appearance preferences.
import { createStoredStore } from '@/lib/store'

const STORAGE_KEY = 'posthaste.developerTools.v1'

/** The store itself — handed to components via PlatformServices (R11). */
export const developerToolsStore = createStoredStore<boolean>({
  key: STORAGE_KEY,
  codec: { read: (raw) => raw === 'true', write: String },
  sync: true,
})

export function isDeveloperToolsEnabled(): boolean {
  return developerToolsStore.get()
}
