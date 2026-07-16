/**
 * First-launch onboarding state: a single localStorage flag recording the
 * highest onboarding version the user has completed. Renderer-owned UI state,
 * mirrors the external-store pattern of `useViewMode`/`useColumnConfig`.
 *
 * The flag is versioned so the brief tour can be re-shown if it changes
 * materially later (bump CURRENT_ONBOARDING_VERSION).
 */
import { useSyncExternalStore } from 'react'

const STORAGE_KEY = 'posthaste.onboarding.completedVersion'

/** Bump when the tour changes enough to warrant re-showing it. */
export const CURRENT_ONBOARDING_VERSION = 1

function readCompletedVersion(): number {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    const parsed = raw ? Number.parseInt(raw, 10) : 0
    return Number.isFinite(parsed) ? parsed : 0
  } catch {
    return 0
  }
}

let cached = readCompletedVersion()
const listeners = new Set<() => void>()

function emit(next: number) {
  cached = next
  try {
    localStorage.setItem(STORAGE_KEY, String(next))
  } catch {
    // Non-fatal: onboarding simply re-shows next launch if storage is blocked.
  }
  for (const listener of listeners) listener()
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

function getSnapshot(): number {
  return cached
}

/** Record the current tour version as completed (finished or skipped). */
export function markOnboardingComplete(): void {
  emit(CURRENT_ONBOARDING_VERSION)
}

/** Reset so the tour shows again (for a future "Replay tutorial" affordance). */
export function restartOnboarding(): void {
  emit(0)
}

/** Reactive: whether the current tour version still needs to be shown. */
export function useOnboardingNeeded(): boolean {
  const completedVersion = useSyncExternalStore(
    subscribe,
    getSnapshot,
    () => CURRENT_ONBOARDING_VERSION,
  )
  return completedVersion < CURRENT_ONBOARDING_VERSION
}
