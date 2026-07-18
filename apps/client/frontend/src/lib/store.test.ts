import { describe, expect, test } from 'bun:test'

import { createStore, createStoredStore, type StorageLike } from './store'

function memoryStorage(initial: Record<string, string> = {}): StorageLike & {
  data: Map<string, string>
} {
  const data = new Map(Object.entries(initial))
  return {
    data,
    getItem: (key) => data.get(key) ?? null,
    setItem: (key, value) => {
      data.set(key, value)
    },
  }
}

describe('createStore', () => {
  test('get returns the initial and then the set value', () => {
    const store = createStore(1)
    expect(store.get()).toBe(1)
    store.set(2)
    expect(store.get()).toBe(2)
  })

  test('set notifies every subscriber, including on same-value sets', () => {
    const store = createStore('a')
    let first = 0
    let second = 0
    store.subscribe(() => first++)
    store.subscribe(() => second++)
    store.set('b')
    store.set('b') // level-triggered: no equality gate
    expect(first).toBe(2)
    expect(second).toBe(2)
  })

  test('teardown removes exactly its own subscription, idempotently', () => {
    const store = createStore(0)
    let kept = 0
    let dropped = 0
    store.subscribe(() => kept++)
    const unsubscribe = store.subscribe(() => dropped++)
    unsubscribe()
    unsubscribe() // second call is a no-op
    store.set(1)
    expect(kept).toBe(1)
    expect(dropped).toBe(0)
  })

  test('onActive runs on 0 -> 1 subscribers and tears down on 1 -> 0', () => {
    let active = 0
    const store = createStore(0, {
      onActive: () => {
        active++
        return () => {
          active--
        }
      },
    })
    const a = store.subscribe(() => {})
    const b = store.subscribe(() => {})
    expect(active).toBe(1)
    a()
    expect(active).toBe(1)
    b()
    expect(active).toBe(0)
    store.subscribe(() => {})
    expect(active).toBe(1)
  })

  test('onActive receives the store and may seed it', () => {
    const store = createStore(0, {
      onActive: (s) => {
        s.set(41 + 1)
        return undefined
      },
    })
    store.subscribe(() => {})
    expect(store.get()).toBe(42)
  })

  test('selector-shaped reads derive from get()', () => {
    // useStore's selector path is `selector(store.get())` per snapshot; the
    // hook itself needs a renderer, so the derivation contract is pinned here.
    const store = createStore({ items: [1, 2, 3] })
    const count = (v: { items: number[] }) => v.items.length
    expect(count(store.get())).toBe(3)
    store.set({ items: [] })
    expect(count(store.get())).toBe(0)
  })
})

describe('createStoredStore', () => {
  test('reads the persisted value through the codec at creation', () => {
    const storage = memoryStorage({ flag: 'true' })
    const store = createStoredStore<boolean>({
      key: 'flag',
      codec: { read: (raw) => raw === 'true', write: String },
      storage,
    })
    expect(store.get()).toBe(true)
  })

  test('absent or corrupt raw yields the codec default', () => {
    const codec = {
      read: (raw: string | null) => {
        if (raw === null) return 0
        const parsed = Number.parseInt(raw, 10)
        return Number.isFinite(parsed) ? parsed : 0
      },
      write: String,
    }
    const empty = createStoredStore<number>({
      key: 'n',
      codec,
      storage: memoryStorage(),
    })
    expect(empty.get()).toBe(0)
    const corrupt = createStoredStore<number>({
      key: 'n',
      codec,
      storage: memoryStorage({ n: 'wat' }),
    })
    expect(corrupt.get()).toBe(0)
  })

  test('set persists through the codec and notifies subscribers', () => {
    const storage = memoryStorage()
    const store = createStoredStore<number>({
      key: 'n',
      codec: { read: (raw) => Number(raw ?? 0), write: String },
      storage,
    })
    let notified = 0
    store.subscribe(() => notified++)
    store.set(7)
    expect(store.get()).toBe(7)
    expect(storage.data.get('n')).toBe('7')
    expect(notified).toBe(1)
  })

  test('missing storage degrades to in-memory state', () => {
    const store = createStoredStore<number>({
      key: 'n',
      codec: { read: (raw) => Number(raw ?? 1), write: String },
      storage: null,
    })
    expect(store.get()).toBe(1)
    store.set(5)
    expect(store.get()).toBe(5)
  })
})
