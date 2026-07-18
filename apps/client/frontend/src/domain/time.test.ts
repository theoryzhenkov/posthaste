import { describe, expect, test } from 'bun:test'

import { parseIsoDate, todayIsoDate } from './time'

describe('parseIsoDate', () => {
  test('accepts a real yyyy-mm-dd date', () => {
    expect<string | null>(parseIsoDate('2026-07-18')).toBe('2026-07-18')
    expect<string | null>(parseIsoDate('2024-02-29')).toBe('2024-02-29') // leap day
  })

  test('is idempotent on its own output', () => {
    const parsed = parseIsoDate('2026-07-18')
    expect(parsed).not.toBeNull()
    expect(parseIsoDate(parsed as string)).toBe(parsed)
  })

  test('rejects wrong shapes and impossible dates', () => {
    expect(parseIsoDate('')).toBeNull()
    expect(parseIsoDate('2026-7-18')).toBeNull()
    expect(parseIsoDate('18-07-2026')).toBeNull()
    expect(parseIsoDate('2026-07-18T00:00:00Z')).toBeNull()
    expect(parseIsoDate('2026-02-30')).toBeNull()
    expect(parseIsoDate('2026-13-01')).toBeNull()
  })
})

describe('todayIsoDate', () => {
  test('formats the UTC calendar date, round-trippable through parseIsoDate', () => {
    const today = todayIsoDate(new Date('2026-07-18T23:59:00.000Z'))
    expect<string>(today).toBe('2026-07-18')
    expect(parseIsoDate(today)).toBe(today)
  })
})
