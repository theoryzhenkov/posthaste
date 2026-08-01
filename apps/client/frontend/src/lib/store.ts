/**
 * R5 — the one store implementation (docs/client/L2-charter.md).
 *
 * `createStore` owns the subscribe/notify/getSnapshot pattern once; every
 * client-local store (preferences, notifications, view mode, column config,
 * onboarding, the transport mirror) builds on it instead of hand-rolling a
 * listener set. React reads a store through `useStore`
 * (useSyncExternalStore); non-React code reads through `get` and writes
 * through `set`. `createStoredStore` adds persistence (via the R8 storage
 * seam, lib/ambient/storage) for preference-shaped state.
 */
import { useSyncExternalStore } from 'react'

import {
  ambientStorage,
  readStorageItem,
  writeStorageItem,
  type StorageLike,
} from './ambient/storage'

export interface Store<T> {
  /** Current value; safe from any code path, React or not. */
  get(): T
  /** Replaces the value and notifies every subscriber — no equality gate, so
   *  level-triggered consumers (cache invalidation) never miss a set. Callers
   *  that want change-detection compare against `get()` first. */
  set(next: T): void
  /** Registers a listener; the returned teardown removes it (tenet VIII). */
  subscribe(listener: () => void): () => void
}

export interface StoreOptions<T> {
  /** Runs when the subscriber count rises 0 -> 1; the returned teardown runs
   *  when it falls back to 0. The seam for cross-window sync and other
   *  keep-fresh machinery that should only live while someone is watching. */
  onActive?: (store: Store<T>) => (() => void) | undefined
}

export function createStore<T>(
  initial: T,
  options?: StoreOptions<T>,
): Store<T> {
  let value = initial
  let teardown: (() => void) | undefined
  const subscribers = new Set<() => void>()
  const store: Store<T> = {
    get: () => value,
    set: (next) => {
      value = next
      for (const notify of [...subscribers]) notify()
    },
    subscribe: (listener) => {
      subscribers.add(listener)
      if (subscribers.size === 1) teardown = options?.onActive?.(store)
      let subscribed = true
      return () => {
        if (!subscribed) return
        subscribed = false
        subscribers.delete(listener)
        if (subscribers.size === 0) {
          teardown?.()
          teardown = undefined
        }
      }
    },
  }
  return store
}

/**
 * React read of a store, optionally through a selector. The selector must be
 * snapshot-stable — return `Object.is`-equal results for the same value
 * (primitives and derived fields, not fresh objects) — or React will
 * re-render on every notification.
 */
export function useStore<T>(store: Store<T>): T
export function useStore<T, U>(store: Store<T>, selector: (value: T) => U): U
export function useStore<T, U>(
  store: Store<T>,
  selector?: (value: T) => U,
): T | U {
  const snapshot = (): T | U =>
    selector ? selector(store.get()) : store.get()
  // The same function serves as the server snapshot: a store is plain
  // in-memory state with no request scope, so a non-browser render has
  // nothing different to read. Without it React refuses to render any
  // store-backed component outside the browser — which is exactly how these
  // components are unit-tested (`renderToStaticMarkup`, no DOM).
  return useSyncExternalStore(store.subscribe, snapshot, snapshot)
}

/** How a stored value crosses the string boundary of web storage. */
interface StorageCodec<T> {
  /** Parse, don't validate (R3): absent or corrupt raw yields a default T. */
  read(raw: string | null): T
  write(value: T): string
}

export interface StoredStoreOptions<T> {
  key: string
  codec: StorageCodec<T>
  /** Mirror other windows: re-read on `storage` events for the key. The
   *  listener lives for the document's lifetime (stored stores are
   *  module-scoped singletons); the window itself is the teardown boundary. */
  sync?: boolean
  /** Defaults to the window's localStorage; absent storage (tests, blocked
   *  access) degrades to in-memory state. */
  storage?: StorageLike | null
}

/**
 * A store persisted under one localStorage key. Reads once at creation,
 * persists on every `set`; persistence is best-effort — blocked storage
 * loses the value across restarts, nothing else.
 */
export function createStoredStore<T>({
  key,
  codec,
  sync,
  storage = ambientStorage(),
}: StoredStoreOptions<T>): Store<T> {
  const read = () => codec.read(readStorageItem(key, storage))
  const inner = createStore(read())
  if (sync === true && typeof window !== 'undefined') {
    window.addEventListener('storage', (event) => {
      if (event.key === key) inner.set(read())
    })
  }
  return {
    get: inner.get,
    subscribe: inner.subscribe,
    set: (next) => {
      writeStorageItem(key, codec.write(next), storage)
      inner.set(next)
    },
  }
}
