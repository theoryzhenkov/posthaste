/**
 * Per-view message-list display mode (flat messages vs conversation tree),
 * persisted to localStorage and keyed by view identity so each opened view
 * remembers its own mode. A `createStoredStore` (R5) shared by every list
 * instance so they all stay in sync.
 */
import { useCallback } from 'react'

import type { MessageListViewMode } from '@/domain/vocabulary'
import { createStoredStore, useStore } from '@/lib/store'

const STORAGE_KEY = 'posthaste-message-view-mode-v1'

type ModeMap = Record<string, MessageListViewMode>

function readModeMap(raw: string | null): ModeMap {
  if (!raw) return {}
  try {
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

const viewModeStore = createStoredStore<ModeMap>({
  key: STORAGE_KEY,
  codec: { read: readModeMap, write: (map) => JSON.stringify(map) },
})

export function useViewMode(viewModeKey: string) {
  const map = useStore(viewModeStore)
  const mode: MessageListViewMode = map[viewModeKey] ?? 'messages'

  const setMode = useCallback(
    (next: MessageListViewMode) => {
      viewModeStore.set({ ...viewModeStore.get(), [viewModeKey]: next })
    },
    [viewModeKey],
  )

  return { mode, setMode } as const
}
