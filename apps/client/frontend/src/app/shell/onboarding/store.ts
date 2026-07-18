/**
 * First-launch onboarding state: a single localStorage flag recording the
 * highest onboarding version the user has completed. Renderer-owned UI state
 * on a `createStoredStore` (R5).
 *
 * The flag is versioned so the brief tour can be re-shown if it changes
 * materially later (bump CURRENT_ONBOARDING_VERSION).
 */
import { createStoredStore, useStore } from '@/lib/store'

const STORAGE_KEY = 'posthaste.onboarding.completedVersion'

/** Bump when the tour changes enough to warrant re-showing it. */
const CURRENT_ONBOARDING_VERSION = 1

const completedVersionStore = createStoredStore<number>({
  key: STORAGE_KEY,
  codec: {
    read: (raw) => {
      const parsed = raw ? Number.parseInt(raw, 10) : 0
      return Number.isFinite(parsed) ? parsed : 0
    },
    write: String,
  },
})

/** Record the current tour version as completed (finished or skipped). */
export function markOnboardingComplete(): void {
  completedVersionStore.set(CURRENT_ONBOARDING_VERSION)
}

/** Reactive: whether the current tour version still needs to be shown. */
export function useOnboardingNeeded(): boolean {
  return useStore(
    completedVersionStore,
    (completedVersion) => completedVersion < CURRENT_ONBOARDING_VERSION,
  )
}
