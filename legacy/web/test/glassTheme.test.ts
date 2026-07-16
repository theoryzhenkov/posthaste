import { describe, expect, it } from 'bun:test'

import {
  appendGlassBloom,
  clampNumber,
  maxGlassBloomCount,
  normalizeGlassThemeParameters,
  removeGlassBloom,
  updateGlassBloom,
} from '../src/design/glassTheme'

function blooms(count: number) {
  return {
    blooms: Array.from({ length: count }, (_, i) => ({
      id: `b${i}`,
      hue: 0,
      x: 0,
      y: 0,
      opacity: 0.1,
      radius: 40,
    })),
  }
}

describe('clampNumber', () => {
  it('coerces strings, clamps to range, and falls back on non-finite input', () => {
    expect(clampNumber(42, 0, 100, 5)).toBe(42)
    expect(clampNumber(150, 0, 100, 5)).toBe(100)
    expect(clampNumber(-10, 0, 100, 5)).toBe(0)
    expect(clampNumber('30', 0, 100, 5)).toBe(30)
    expect(clampNumber('abc', 0, 100, 5)).toBe(5)
    expect(clampNumber(undefined, 0, 100, 5)).toBe(5)
  })
})

describe('glass theme normalization', () => {
  it('clamps each field to its valid range and normalizes the hue', () => {
    const { blooms: out } = normalizeGlassThemeParameters({
      blooms: [{ id: 'b', hue: 405, x: 200, y: -5, opacity: 5, radius: 10 }],
    })
    expect(out).toHaveLength(1)
    expect(out[0]).toMatchObject({
      hue: 45, // 405 wrapped
      x: 100, // clamped 0..100
      y: 0,
      opacity: 0.5, // clamped 0..0.5
      radius: 25, // clamped 25..70
    })
  })

  it('caps bloom count at the maximum and guarantees at least one', () => {
    expect(normalizeGlassThemeParameters(blooms(12)).blooms).toHaveLength(
      maxGlassBloomCount,
    )
    expect(normalizeGlassThemeParameters({ blooms: [] }).blooms).toHaveLength(1)
  })

  it('de-duplicates bloom ids', () => {
    const out = normalizeGlassThemeParameters({
      blooms: [
        { id: 'dup', hue: 0, x: 0, y: 0, opacity: 0.1, radius: 40 },
        { id: 'dup', hue: 0, x: 0, y: 0, opacity: 0.1, radius: 40 },
      ],
    })
    expect(new Set(out.blooms.map((b) => b.id)).size).toBe(2)
  })
})

describe('glass bloom add/remove bounds', () => {
  it('appends below the cap but refuses to exceed it', () => {
    const one = normalizeGlassThemeParameters(blooms(1))
    expect(appendGlassBloom(one).parameters.blooms).toHaveLength(2)

    const full = normalizeGlassThemeParameters(blooms(maxGlassBloomCount))
    const appended = appendGlassBloom(full)
    expect(appended.parameters.blooms).toHaveLength(maxGlassBloomCount)
  })

  it('removes a bloom but never drops below one', () => {
    const two = normalizeGlassThemeParameters(blooms(2))
    const removed = removeGlassBloom(two, two.blooms[0]!.id)
    expect(removed.blooms).toHaveLength(1)
    expect(removed.blooms[0]!.id).toBe(two.blooms[1]!.id)

    const one = normalizeGlassThemeParameters(blooms(1))
    expect(removeGlassBloom(one, one.blooms[0]!.id).blooms).toHaveLength(1)
  })

  it('patches the targeted bloom and clamps patched values', () => {
    const params = normalizeGlassThemeParameters(blooms(2))
    const id = params.blooms[0]!.id
    const updated = updateGlassBloom(params, id, { opacity: 5, x: 42 })
    const target = updated.blooms.find((b) => b.id === id)!
    expect(target.opacity).toBe(0.5)
    expect(target.x).toBe(42)
  })
})
