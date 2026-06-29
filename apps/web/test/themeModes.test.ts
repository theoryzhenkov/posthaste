import { describe, expect, it } from 'bun:test'

import { isBuiltInThemeId, isThemeMode, themeStyle } from '../src/design/theme'

describe('theme mode + theme-id guards', () => {
  it('recognizes valid theme modes', () => {
    expect(isThemeMode('light')).toBe(true)
    expect(isThemeMode('dark')).toBe(true)
    expect(isThemeMode('system')).toBe(true)
    expect(isThemeMode('bogus')).toBe(false)
  })

  it('recognizes the built-in theme ids', () => {
    expect(isBuiltInThemeId('neutral')).toBe(true)
    expect(isBuiltInThemeId('glass')).toBe(true)
    expect(isBuiltInThemeId('paperInk')).toBe(false)
    expect(isBuiltInThemeId('my-custom-theme')).toBe(false)
  })
})

describe('themeStyle', () => {
  it('maps built-in ids to their structural style', () => {
    expect(themeStyle('neutral')).toBe('neutral')
    expect(themeStyle('glass')).toBe('glass')
  })

  it('falls back to the neutral base style for an unknown (user) theme', () => {
    expect(themeStyle('my-custom-theme')).toBe('neutral')
  })
})
