import { describe, expect, it } from 'bun:test'

import { normalizeProgressValue } from '../src/progressValue'

describe('progress value normalization', () => {
  it('keeps finite percentages inside the supported range', () => {
    expect(normalizeProgressValue(42)).toBe(42)
    expect(normalizeProgressValue(0)).toBe(0)
    expect(normalizeProgressValue(100)).toBe(100)
  })

  it('clamps out-of-range determinate percentages', () => {
    expect(normalizeProgressValue(-20)).toBe(0)
    expect(normalizeProgressValue(140)).toBe(100)
  })

  it('treats absent or non-finite values as indeterminate progress', () => {
    expect(normalizeProgressValue(null)).toBeNull()
    expect(normalizeProgressValue(undefined)).toBeNull()
    expect(normalizeProgressValue(Number.NaN)).toBeNull()
  })
})
