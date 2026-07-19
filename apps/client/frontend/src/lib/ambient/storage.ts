/**
 * The web-storage seam (R8, docs/client/L2-charter.md): ambient
 * `window.localStorage` is touched here (and in the preferences store's
 * multi-key persistence, data/preferences) — nowhere else; the eslint
 * ratchet bans it. Preference-shaped state should sit one level up, on
 * `createStoredStore` (lib/store); these functions are for the raw
 * string-per-key cases (panel geometry, palette recents, repair
 * breadcrumbs) and are best-effort throughout: absent or blocked storage
 * reads as null and swallows writes.
 */

/** The subset of Storage the seam needs; injectable for tests. */
export type StorageLike = Pick<Storage, 'getItem' | 'setItem'>

/** The window's localStorage, or null when absent (tests) or blocked. */
export function ambientStorage(): StorageLike | null {
  if (typeof window === 'undefined') return null
  try {
    return window.localStorage
  } catch {
    return null
  }
}

export function readStorageItem(
  key: string,
  storage: StorageLike | null = ambientStorage(),
): string | null {
  try {
    return storage?.getItem(key) ?? null
  } catch {
    return null
  }
}

export function writeStorageItem(
  key: string,
  value: string,
  storage: StorageLike | null = ambientStorage(),
): void {
  try {
    storage?.setItem(key, value)
  } catch {
    // Best-effort persistence; see the module doc.
  }
}

export function removeStorageItem(key: string): void {
  try {
    if (typeof window !== 'undefined') window.localStorage.removeItem(key)
  } catch {
    // Best-effort persistence; see the module doc.
  }
}

/** Wipe ALL client-local storage — the factory-reset hammer (desktop
 *  repair), not a per-key remove. */
export function clearAmbientStorage(): void {
  try {
    if (typeof window !== 'undefined') window.localStorage.clear()
  } catch {
    // Best-effort; a blocked wipe leaves stale prefs, nothing worse.
  }
}
