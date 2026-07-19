import { describe, expect, test } from 'bun:test'

import { newId, randomInt } from './random'

describe('random seam', () => {
  test('newId is 26 chars of Crockford base32 and unique', () => {
    const a = newId()
    const b = newId()
    expect(a).toMatch(/^[0-9A-HJKMNP-TV-Z]{26}$/)
    expect(a).not.toBe(b)
  })

  test('newId sorts by mint order (timestamp prefix)', () => {
    const a = newId()
    const b = newId()
    expect(a.slice(0, 10) <= b.slice(0, 10)).toBe(true)
  })

  test('randomInt stays in [0, bound)', () => {
    for (let i = 0; i < 100; i++) {
      const n = randomInt(5)
      expect(n).toBeGreaterThanOrEqual(0)
      expect(n).toBeLessThan(5)
      expect(Number.isInteger(n)).toBe(true)
    }
  })
})
