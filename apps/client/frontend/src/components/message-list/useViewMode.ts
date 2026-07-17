/**
 * Per-view message-list display mode (flat messages vs conversation tree),
 * persisted to localStorage and keyed by view identity so each opened view
 * remembers its own mode. Mirrors the external-store pattern of
 * `useColumnConfig` so all list instances stay in sync.
 *
 */
import { useCallback, useSyncExternalStore } from 'react'

export type MessageListViewMode = 'messages' | 'conversations'

const STORAGE_KEY = 'posthaste-message-view-mode-v1'

type ModeMap = Record<string, MessageListViewMode>

function readFromStorage(): ModeMap {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return {}
    const parsed: unknown = JSON.parse(raw)
    if (typeof parsed !== 'object' || parsed === null) return {}
    const result: ModeMap = {}
    for (const [key, value] of Object.entries(
      parsed as Record<string, unknown>,
    )) {
      if (value === 'messages' || value === 'conversations') {
        result[key] = value
      }
    }
    return result
  } catch {
    return {}
  }
}

let cached: ModeMap = readFromStorage()
const listeners = new Set<() => void>()

function subscribe(listener: () => void): () => void {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

function getSnapshot(): ModeMap {
  return cached
}

function persist(next: ModeMap) {
  cached = next
  localStorage.setItem(STORAGE_KEY, JSON.stringify(next))
  for (const listener of listeners) listener()
}

export function useViewMode(viewModeKey: string) {
  const map = useSyncExternalStore(subscribe, getSnapshot)
  const mode: MessageListViewMode = map[viewModeKey] ?? 'messages'

  const setMode = useCallback(
    (next: MessageListViewMode) => {
      persist({ ...getSnapshot(), [viewModeKey]: next })
    },
    [viewModeKey],
  )

  return { mode, setMode } as const
}
