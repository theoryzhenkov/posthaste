import { afterEach, describe, expect, test } from 'bun:test'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

import {
  acquireWindowSlot,
  raiseWindowSlot,
  releaseWindowSlot,
  Z,
  type WindowSlot,
} from './layering'

/** The CSS mirror of the layering scale (`--z-base` … `--z-tooltip`). */
const CSS_PATH = join(import.meta.dir, '../../app/assets/index.css')

describe('layering scale CSS drift', () => {
  test('every Z tier is mirrored as a --z-* custom property with the same value', () => {
    const css = readFileSync(CSS_PATH, 'utf8')
    for (const [tier, value] of Object.entries(Z)) {
      const property = `--z-${tier.toLowerCase()}`
      const match = css.match(new RegExp(`${property}:\\s*(\\d+)\\s*;`))
      expect(match?.[1], `${property} missing from index.css`).toBeDefined()
      expect(Number(match?.[1]), `${property} drifted from Z.${tier}`).toBe(
        value,
      )
    }
  })
})

describe('WINDOW band allocator', () => {
  // The allocator is module-global, as the real band is. Every test releases
  // what it claims so one test's live panels cannot skew the next.
  const claimed = new Set<WindowSlot>()

  function open(): WindowSlot {
    const slot = acquireWindowSlot((z) => {
      slot.z = z
    })
    claimed.add(slot)
    return slot
  }

  function close(slot: WindowSlot): void {
    releaseWindowSlot(slot)
    claimed.delete(slot)
  }

  afterEach(() => {
    for (const slot of claimed) {
      releaseWindowSlot(slot)
    }
    claimed.clear()
  })

  test('a newly opened panel sits above its peers', () => {
    const first = open()
    const second = open()
    const third = open()
    expect(second.z).toBeGreaterThan(first.z)
    expect(third.z).toBeGreaterThan(second.z)
  })

  test('raising lifts a panel above every other live panel', () => {
    const first = open()
    const second = open()
    raiseWindowSlot(first)
    expect(first.z).toBeGreaterThan(second.z)
  })

  test('every slot stays inside the band, strictly below OVERLAY', () => {
    const slots = [open(), open(), open()]
    for (const slot of slots) {
      raiseWindowSlot(slot)
      expect(slot.z).toBeGreaterThanOrEqual(Z.WINDOW)
      expect(slot.z).toBeLessThan(Z.OVERLAY)
    }
  })

  test('releasing a slot frees its position for the next panel', () => {
    const first = open()
    const second = open()
    close(second)
    const third = open()
    // Reuses the band rather than climbing past the released value forever.
    expect(third.z).toBe(second.z)
    expect(third.z).toBeGreaterThan(first.z)
  })

  test('the band survives far more interactions than it has slots', () => {
    // The regression this allocator exists for. The previous monotonic counter
    // burned a slot per pointer-down, so ~900 interactions pinned every panel
    // to the ceiling, where they tied and bring-to-front stopped working.
    const a = open()
    const b = open()
    const c = open()
    for (let i = 0; i < 10_000; i++) {
      const slot = [a, b, c][i % 3]!
      raiseWindowSlot(slot)
      expect(slot.z).toBeLessThan(Z.OVERLAY)
    }
    // Order is still strict and still reflects the raise order, not a tie.
    raiseWindowSlot(a)
    raiseWindowSlot(b)
    raiseWindowSlot(c)
    expect(new Set([a.z, b.z, c.z]).size).toBe(3)
    expect(c.z).toBeGreaterThan(b.z)
    expect(b.z).toBeGreaterThan(a.z)
  })

  test('re-seating tells the holder its z moved', () => {
    // A panel's z can change without that panel doing anything, so the callback
    // is the only way it learns. Without it, re-seating would desync the DOM.
    const moves: number[] = []
    const watched = acquireWindowSlot((z) => {
      watched.z = z
      moves.push(z)
    })
    claimed.add(watched)
    const other = open()
    for (let i = 0; i < 2_000; i++) {
      raiseWindowSlot(other)
      raiseWindowSlot(watched)
    }
    expect(moves.length).toBeGreaterThan(0)
    expect(watched.z).toBeLessThan(Z.OVERLAY)
  })

  test('a released slot ignores further raises', () => {
    const slot = open()
    const previous = slot.z
    close(slot)
    raiseWindowSlot(slot)
    expect(slot.z).toBe(previous)
  })
})
