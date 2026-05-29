import { describe, expect, it } from 'bun:test'

import {
  isPalettePresetId,
  isThemeMode,
  resolvePaletteMode,
} from '../src/design/theme'

describe('theme mode guards', () => {
  it('recognizes valid theme modes and palette preset ids', () => {
    expect(isThemeMode('light')).toBe(true)
    expect(isThemeMode('dark')).toBe(true)
    expect(isThemeMode('system')).toBe(true)
    expect(isThemeMode('bogus')).toBe(false)

    expect(isPalettePresetId('neutral')).toBe(true)
    expect(isPalettePresetId('glass')).toBe(true)
    expect(isPalettePresetId('not-a-preset')).toBe(false)
  })
})

describe('resolvePaletteMode', () => {
  it('keeps the requested mode when the palette supports it', () => {
    expect(resolvePaletteMode('neutral', 'light')).toBe('light')
    expect(resolvePaletteMode('neutral', 'dark')).toBe('dark')
  })

  it('falls back to the palette’s primary mode when the requested one is unsupported', () => {
    // acid is dark-only; paperInk/marzipan are light-only
    expect(resolvePaletteMode('acid', 'light')).toBe('dark')
    expect(resolvePaletteMode('paperInk', 'dark')).toBe('light')
    expect(resolvePaletteMode('marzipan', 'dark')).toBe('light')
  })
})
