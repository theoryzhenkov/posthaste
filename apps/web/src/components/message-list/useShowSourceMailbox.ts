/**
 * Per-view "show source mailbox" row-chip toggle. Mirrors `useViewMode`'s
 * external-store pattern (localStorage-persisted, keyed by view identity,
 * `useSyncExternalStore`-synced across instances) so both toggles behave
 * identically to the user.
 *
 * Unlike `useViewMode`, an unset key falls back to a caller-supplied default
 * (ON for aggregate views, OFF for a single source mailbox — see
 * `isAggregateMessageView`) instead of a fixed value, so the persisted map
 * only ever records explicit user overrides, and the default can keep
 * evolving without stomping on a choice the user already made for a view.
 *
 * @spec docs/L1-ui#messagelist
 */
import { useCallback, useSyncExternalStore } from 'react'

const STORAGE_KEY = 'posthaste-show-source-mailbox-v1'

type OverrideMap = Record<string, boolean>

function readFromStorage(): OverrideMap {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return {}
    const parsed: unknown = JSON.parse(raw)
    if (typeof parsed !== 'object' || parsed === null) return {}
    const result: OverrideMap = {}
    for (const [key, value] of Object.entries(
      parsed as Record<string, unknown>,
    )) {
      if (typeof value === 'boolean') {
        result[key] = value
      }
    }
    return result
  } catch {
    return {}
  }
}

let cached: OverrideMap = readFromStorage()
const listeners = new Set<() => void>()

function subscribe(listener: () => void): () => void {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

function getSnapshot(): OverrideMap {
  return cached
}

function persist(next: OverrideMap) {
  cached = next
  localStorage.setItem(STORAGE_KEY, JSON.stringify(next))
  for (const listener of listeners) listener()
}

/** Test-only: reload the in-memory cache from (a possibly-cleared)
 *  localStorage, so tests don't leak overrides set by earlier tests. */
export function resetShowSourceMailboxForTesting(): void {
  cached = readFromStorage()
}

export function useShowSourceMailbox(viewKey: string, defaultValue: boolean) {
  const map = useSyncExternalStore(subscribe, getSnapshot)
  const show = map[viewKey] ?? defaultValue

  const setShow = useCallback(
    (next: boolean) => {
      persist({ ...getSnapshot(), [viewKey]: next })
    },
    [viewKey],
  )

  const toggleShow = useCallback(() => {
    setShow(!show)
  }, [setShow, show])

  return { show, setShow, toggleShow } as const
}
