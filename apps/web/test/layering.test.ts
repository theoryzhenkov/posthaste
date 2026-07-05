import { describe, expect, it, beforeEach } from 'bun:test'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

import {
  nextWindowZIndex,
  resetWindowStacking,
  WINDOW_BAND_MAX,
  WINDOW_BAND_MIN,
  Z,
} from '../src/layering'

describe('layering scale', () => {
  it('tiers ascend BASE < RAISED < SURFACE < POPOVER < WINDOW < OVERLAY < MODAL < TOAST < TOOLTIP', () => {
    const order = [
      Z.BASE,
      Z.RAISED,
      Z.SURFACE,
      Z.POPOVER,
      Z.WINDOW,
      Z.OVERLAY,
      Z.MODAL,
      Z.TOAST,
      Z.TOOLTIP,
    ]
    for (let i = 1; i < order.length; i += 1) {
      expect(order[i]).toBeGreaterThan(order[i - 1])
    }
  })

  it('places the command palette (OVERLAY) and dialogs (MODAL) above compose windows (WINDOW)', () => {
    // The two reported bugs, expressed as tier invariants.
    expect(Z.OVERLAY).toBeGreaterThan(Z.WINDOW)
    expect(Z.MODAL).toBeGreaterThan(Z.WINDOW)
    expect(Z.MODAL).toBeGreaterThan(Z.OVERLAY)
  })

  it('bounds the WINDOW band strictly below OVERLAY', () => {
    expect(WINDOW_BAND_MIN).toBe(Z.WINDOW)
    expect(WINDOW_BAND_MAX).toBeLessThan(Z.OVERLAY)
    expect(WINDOW_BAND_MAX).toBeGreaterThan(WINDOW_BAND_MIN)
  })
})

describe('window bring-to-front allocator', () => {
  beforeEach(() => resetWindowStacking())

  it('raises each newly opened/focused window above the previous one', () => {
    const first = nextWindowZIndex()
    const second = nextWindowZIndex()
    const third = nextWindowZIndex()
    expect(second).toBeGreaterThan(first)
    expect(third).toBeGreaterThan(second)
  })

  it('always stays inside the band and below OVERLAY', () => {
    for (let i = 0; i < 5000; i += 1) {
      const z = nextWindowZIndex()
      expect(z).toBeGreaterThanOrEqual(WINDOW_BAND_MIN)
      expect(z).toBeLessThanOrEqual(WINDOW_BAND_MAX)
      expect(z).toBeLessThan(Z.OVERLAY)
    }
  })
})

describe('CSS custom properties mirror the TS scale (no drift)', () => {
  it('every --z-* var in index.css matches its Z tier', () => {
    const css = readFileSync(
      fileURLToPath(new URL('../src/index.css', import.meta.url)),
      'utf8',
    )
    const expected: Record<string, number> = {
      '--z-base': Z.BASE,
      '--z-raised': Z.RAISED,
      '--z-surface': Z.SURFACE,
      '--z-popover': Z.POPOVER,
      '--z-window': Z.WINDOW,
      '--z-overlay': Z.OVERLAY,
      '--z-modal': Z.MODAL,
      '--z-toast': Z.TOAST,
      '--z-tooltip': Z.TOOLTIP,
    }
    for (const [name, value] of Object.entries(expected)) {
      const match = new RegExp(`${name}:\\s*(\\d+)\\s*;`).exec(css)
      expect(match, `${name} declared in index.css`).not.toBeNull()
      expect(Number(match![1])).toBe(value)
    }
  })
})
