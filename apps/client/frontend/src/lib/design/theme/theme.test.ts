import { describe, expect, test } from 'bun:test'

import { parseUiDensity, uiDensities } from '../tokens/density'
import { parseThemeMode, themeModes, themeStyle } from './theme'

describe('parseThemeMode', () => {
  test('accepts every mode, is idempotent, rejects the rest', () => {
    for (const mode of themeModes) {
      expect(parseThemeMode(mode)).toBe(mode)
      expect(parseThemeMode(parseThemeMode(mode) as string)).toBe(mode)
    }
    expect(parseThemeMode('')).toBeNull()
    expect(parseThemeMode('Dark')).toBeNull()
    expect(parseThemeMode('sepia')).toBeNull()
  })
})

describe('parseUiDensity', () => {
  test('accepts every density, is idempotent, rejects the rest', () => {
    for (const density of uiDensities) {
      expect(parseUiDensity(density)).toBe(density)
      expect(parseUiDensity(parseUiDensity(density) as string)).toBe(density)
    }
    expect(parseUiDensity('')).toBeNull()
    expect(parseUiDensity('Compact')).toBeNull()
    expect(parseUiDensity('roomy')).toBeNull()
  })
})

describe('themeStyle', () => {
  test('unknown theme ids fall back to the neutral style', () => {
    expect(themeStyle('glass')).toBe('glass')
    expect(themeStyle('neutral')).toBe('neutral')
    expect(themeStyle('user-import-42')).toBe('neutral')
  })
})
