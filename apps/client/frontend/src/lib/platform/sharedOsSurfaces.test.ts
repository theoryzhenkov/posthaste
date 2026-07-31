import { describe, expect, test } from 'bun:test'

import {
  createSharedOsSurfaceClaim,
  type SharedOsSurfaceClaimOptions,
} from './sharedOsSurfaces'
import type { StorageLike } from '@/lib/ambient/storage'

const LEASE_MS = 5_000

/** One shared localStorage, as several windows see it. */
function sharedStorage(): StorageLike {
  const entries = new Map<string, string>()
  return {
    getItem: (key) => entries.get(key) ?? null,
    setItem: (key, value) => {
      entries.set(key, value)
    },
  }
}

/** A window on that storage, with the clock under the test's control. */
function windowOn(
  storage: StorageLike,
  holderId: string,
  clock: { ms: number },
  extra?: SharedOsSurfaceClaimOptions,
) {
  return createSharedOsSurfaceClaim({
    holderId,
    storage,
    now: () => clock.ms,
    leaseMs: LEASE_MS,
    ...extra,
  })
}

/** Three windows of one app, sharing a storage and a clock, none having
 *  polled yet. `startMs` seeds the clock so a test can assert on absolute
 *  deadlines. */
function appWindows(startMs = 0) {
  const storage = sharedStorage()
  const clock = { ms: startMs }
  return {
    storage,
    clock,
    main: windowOn(storage, 'main', clock),
    surface: windowOn(storage, 'surface', clock),
    third: windowOn(storage, 'third', clock),
  }
}

describe('shared OS surface claim', () => {
  test('the first window to poll owns the surfaces', () => {
    const { main } = appWindows()
    expect(main.poll()).toBe(true)
  })

  test('a second window does not own them while the lease is live', () => {
    const { main, surface } = appWindows()
    expect(main.poll()).toBe(true)
    expect(surface.poll()).toBe(false)
  })

  test('the holder keeps the claim across renewals, past the first lease', () => {
    const { clock, main, surface } = appWindows()

    main.poll()
    // Renew every 2s for 20s — four full lease lengths.
    for (let elapsed = 2_000; elapsed <= 20_000; elapsed += 2_000) {
      clock.ms = elapsed
      expect(main.poll()).toBe(true)
      expect(surface.poll()).toBe(false)
    }
  })

  test('a holder that stops renewing loses the claim once the lease expires', () => {
    const { clock, main, surface } = appWindows(1_000)
    expect(main.poll()).toBe(true)

    // The main window is closed: no more polls, no unload handler, nothing.
    clock.ms = 1_000 + LEASE_MS - 1
    expect(surface.poll()).toBe(false)
    clock.ms = 1_000 + LEASE_MS
    expect(surface.poll()).toBe(true)
  })

  test('the surface window that took over then holds it against the rest', () => {
    const { clock, main, surface, third } = appWindows()

    main.poll()
    clock.ms = LEASE_MS
    expect(surface.poll()).toBe(true)
    expect(third.poll()).toBe(false)
  })

  test('release hands the claim over without waiting out the lease', () => {
    const { main, surface } = appWindows()
    expect(main.poll()).toBe(true)
    main.release()
    expect(surface.poll()).toBe(true)
  })

  test('release by a window that does not hold the claim disturbs nothing', () => {
    const { main, surface } = appWindows()
    expect(main.poll()).toBe(true)
    surface.poll()
    surface.release()
    expect(main.poll()).toBe(true)
    expect(surface.poll()).toBe(false)
  })

  test.each([
    ['not JSON', 'not json at all'],
    ['a JSON scalar', '42'],
    ['null', 'null'],
    ['a record with no holder', '{"expiresAt":9999999}'],
    ['a record with a non-numeric expiry', '{"holder":"x","expiresAt":"soon"}'],
  ])('a claim record that is %s reads as vacant', (_label, raw) => {
    const { storage, main } = appWindows(1_000)
    storage.setItem('posthaste.osSurfaces.claim.v1', raw)
    expect(main.poll()).toBe(true)
  })

  test('a window with no storage owns the surfaces rather than nothing', () => {
    const lone = createSharedOsSurfaceClaim({
      holderId: 'lone',
      storage: null,
      now: () => 1_000,
    })
    expect(lone.poll()).toBe(true)
    lone.release()
    expect(lone.poll()).toBe(true)
  })

  test('a peer that overwrites between the write and the read-back wins', () => {
    // The race storage has no compare-and-swap: this simulates a peer landing
    // its own claim in the instant between our setItem and our read-back.
    const inner = sharedStorage()
    let interlopeOnNextRead = false
    const racy: StorageLike = {
      getItem: (key) => {
        if (interlopeOnNextRead) {
          interlopeOnNextRead = false
          inner.setItem(
            key,
            JSON.stringify({ holder: 'peer', expiresAt: 99_000 }),
          )
        }
        return inner.getItem(key)
      },
      setItem: (key, value) => {
        inner.setItem(key, value)
        interlopeOnNextRead = true
      },
    }

    const clock = { ms: 1_000 }
    expect(windowOn(racy, 'main', clock).poll()).toBe(false)
  })
})
