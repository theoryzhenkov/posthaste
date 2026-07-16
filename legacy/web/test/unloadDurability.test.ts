/**
 * W3 / N18: `visibilitychange` -> hidden and `pagehide` must flush the active
 * entity-store adapter's queued durable writes. `entityStoreAdapter.test.ts`
 * already proves `flush()`/`flushActiveEntityStore()` actually waits on the
 * queue (the durability mechanism); this file proves the DOM wiring calls it
 * at the right moments.
 */
import { afterEach, beforeEach, describe, expect, it } from 'bun:test'

import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

describe('installUnloadDurabilityHooks', () => {
  let installUnloadDurabilityHooks: typeof import('../src/runtime/replica/unloadDurability').installUnloadDurabilityHooks
  let resetUnloadDurabilityHooksForTesting: typeof import('../src/runtime/replica/unloadDurability').resetUnloadDurabilityHooksForTesting

  beforeEach(async () => {
    const mod = await import('../src/runtime/replica/unloadDurability')
    installUnloadDurabilityHooks = mod.installUnloadDurabilityHooks
    resetUnloadDurabilityHooksForTesting =
      mod.resetUnloadDurabilityHooksForTesting
    resetUnloadDurabilityHooksForTesting()
  })

  afterEach(() => {
    resetUnloadDurabilityHooksForTesting()
  })

  it('flushes on visibilitychange -> hidden', async () => {
    const entityStoreAdapter =
      await import('../src/runtime/replica/entityStoreAdapter')
    let flushed = 0
    entityStoreAdapter.__setActiveFlushForTesting(async () => {
      flushed += 1
    })

    installUnloadDurabilityHooks()

    Object.defineProperty(document, 'visibilityState', {
      value: 'hidden',
      configurable: true,
    })
    document.dispatchEvent(new Event('visibilitychange'))
    // The flush is fire-and-forget from the listener; give its microtask a
    // turn.
    await Promise.resolve()

    expect(flushed).toBe(1)
    entityStoreAdapter.__setActiveFlushForTesting(undefined)
  })

  it('does NOT flush on visibilitychange -> visible', async () => {
    const entityStoreAdapter =
      await import('../src/runtime/replica/entityStoreAdapter')
    let flushed = 0
    entityStoreAdapter.__setActiveFlushForTesting(async () => {
      flushed += 1
    })

    installUnloadDurabilityHooks()

    Object.defineProperty(document, 'visibilityState', {
      value: 'visible',
      configurable: true,
    })
    document.dispatchEvent(new Event('visibilitychange'))
    await Promise.resolve()

    expect(flushed).toBe(0)
    entityStoreAdapter.__setActiveFlushForTesting(undefined)
  })

  it('flushes on pagehide', async () => {
    const entityStoreAdapter =
      await import('../src/runtime/replica/entityStoreAdapter')
    let flushed = 0
    entityStoreAdapter.__setActiveFlushForTesting(async () => {
      flushed += 1
    })

    installUnloadDurabilityHooks()

    window.dispatchEvent(new Event('pagehide'))
    await Promise.resolve()

    expect(flushed).toBe(1)
    entityStoreAdapter.__setActiveFlushForTesting(undefined)
  })

  it('is idempotent: a second install does not double-register listeners', async () => {
    const entityStoreAdapter =
      await import('../src/runtime/replica/entityStoreAdapter')
    let flushed = 0
    entityStoreAdapter.__setActiveFlushForTesting(async () => {
      flushed += 1
    })

    installUnloadDurabilityHooks()
    installUnloadDurabilityHooks()

    window.dispatchEvent(new Event('pagehide'))
    await Promise.resolve()

    expect(flushed).toBe(1)
    entityStoreAdapter.__setActiveFlushForTesting(undefined)
  })
})
