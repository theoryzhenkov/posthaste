import { describe, expect, test } from 'bun:test'

import {
  parseEmailAddress,
  parseEmailPattern,
  patternEmailAddress,
  patternMatchesEmail,
} from './address'

describe('parseEmailAddress', () => {
  test('accepts a concrete address and trims it', () => {
    expect<string | null>(parseEmailAddress('theo@example.com')).toBe('theo@example.com')
    expect<string | null>(parseEmailAddress('  theo@example.com  ')).toBe('theo@example.com')
  })

  test('is idempotent on its own output', () => {
    const parsed = parseEmailAddress('theo@example.com')
    expect(parsed).not.toBeNull()
    expect(parseEmailAddress(parsed as string)).toBe(parsed)
  })

  test('rejects empty, wildcard, and malformed input', () => {
    expect(parseEmailAddress('')).toBeNull()
    expect(parseEmailAddress('   ')).toBeNull()
    expect(parseEmailAddress('*@example.com')).toBeNull()
    expect(parseEmailAddress('theo*@example.com')).toBeNull()
    expect(parseEmailAddress('no-at-sign')).toBeNull()
    expect(parseEmailAddress('two@at@signs')).toBeNull()
    expect(parseEmailAddress('spaces in@example.com')).toBeNull()
  })
})

describe('parseEmailPattern', () => {
  test('accepts a concrete address and a *@domain catch-all', () => {
    expect<string | null>(parseEmailPattern('theo@example.com')).toBe('theo@example.com')
    expect<string | null>(parseEmailPattern(' *@corp.example.com ')).toBe(
      '*@corp.example.com',
    )
  })

  test('is idempotent on its own output', () => {
    const parsed = parseEmailPattern('*@corp.example.com')
    expect(parsed).not.toBeNull()
    expect(parseEmailPattern(parsed as string)).toBe(parsed)
  })

  test('rejects empty input and non-catch-all wildcard shapes', () => {
    expect(parseEmailPattern('')).toBeNull()
    expect(parseEmailPattern('*@')).toBeNull()
    expect(parseEmailPattern('theo*@example.com')).toBeNull()
    expect(parseEmailPattern('*@two@ats')).toBeNull()
    expect(parseEmailPattern('no-at-sign')).toBeNull()
  })
})

describe('patternEmailAddress', () => {
  test('returns the concrete address, null for a catch-all', () => {
    const concrete = parseEmailPattern('theo@example.com')
    const wildcard = parseEmailPattern('*@corp.example.com')
    expect<string | null | false>(
      concrete && patternEmailAddress(concrete),
    ).toBe('theo@example.com')
    expect(wildcard && patternEmailAddress(wildcard)).toBeNull()
  })
})

describe('patternMatchesEmail', () => {
  test('concrete patterns compare case-insensitively', () => {
    const pattern = parseEmailPattern('Theo@Example.com')
    expect(pattern).not.toBeNull()
    expect(patternMatchesEmail(pattern!, ' theo@example.COM ')).toBe(true)
    expect(patternMatchesEmail(pattern!, 'other@example.com')).toBe(false)
  })

  test('catch-alls match the domain suffix only', () => {
    const pattern = parseEmailPattern('*@corp.example.com')
    expect(pattern).not.toBeNull()
    expect(patternMatchesEmail(pattern!, 'anyone@corp.example.com')).toBe(true)
    expect(patternMatchesEmail(pattern!, 'anyone@example.com')).toBe(false)
  })
})
