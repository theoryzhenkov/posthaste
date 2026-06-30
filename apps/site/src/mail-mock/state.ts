import type { PersistedMockState } from './types'

export const MOCK_STATE_STORAGE_KEY = 'posthaste-site-mail-mock-state-v1'

export function loadPersistedMockState(): PersistedMockState {
  if (typeof window === 'undefined') return {}

  const raw = window.localStorage.getItem(MOCK_STATE_STORAGE_KEY)
  if (!raw) return {}

  try {
    const parsed = JSON.parse(raw) as PersistedMockState
    return parsed && typeof parsed === 'object' ? parsed : {}
  } catch {
    return {}
  }
}

export function persistedSet(
  values: string[] | undefined,
): ReadonlySet<string> {
  return new Set(values ?? [])
}
