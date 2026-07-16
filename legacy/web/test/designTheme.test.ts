import { describe, expect, it } from 'bun:test'

import {
  accentColor,
  defaultAccentHue,
  normalizeAccentHue,
  parseAccentHue,
} from '../src/design/accent'
import { isUiDensity } from '../src/design/density'

describe('accent hue', () => {
  it('normalizes into [0,360) with wrap-around and rounding', () => {
    expect(normalizeAccentHue(45)).toBe(45)
    expect(normalizeAccentHue(360)).toBe(0)
    expect(normalizeAccentHue(405)).toBe(45)
    expect(normalizeAccentHue(-30)).toBe(330)
    expect(normalizeAccentHue(44.6)).toBe(45)
  })

  it('falls back to the default hue for non-finite values', () => {
    expect(normalizeAccentHue(Number.NaN)).toBe(defaultAccentHue)
    expect(normalizeAccentHue(Number.POSITIVE_INFINITY)).toBe(defaultAccentHue)
  })

  it('parses string hues, defaulting on null and non-numeric input', () => {
    expect(parseAccentHue(null)).toBe(defaultAccentHue)
    expect(parseAccentHue('120')).toBe(120)
    expect(parseAccentHue('-30')).toBe(330)
    expect(parseAccentHue('abc')).toBe(defaultAccentHue)
    // empty string coerces to 0 via Number(''), which is a valid hue
    expect(parseAccentHue('')).toBe(0)
  })

  it('renders an oklch color with a normalized hue and overridable L/C', () => {
    expect(accentColor(45)).toBe('oklch(0.68 0.17 45)')
    expect(accentColor(405)).toBe('oklch(0.68 0.17 45)')
    expect(accentColor(200, 0.5, 0.1)).toBe('oklch(0.5 0.1 200)')
  })
})

describe('ui density guard', () => {
  it('recognizes only the known density tokens', () => {
    expect(isUiDensity('compact')).toBe(true)
    expect(isUiDensity('cozy')).toBe(true)
    expect(isUiDensity('comfortable')).toBe(true)
    expect(isUiDensity('huge')).toBe(false)
    expect(isUiDensity('')).toBe(false)
  })
})
