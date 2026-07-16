import { describe, expect, it } from 'bun:test'

import {
  getSystemThemeMode,
  resolveThemeMode,
} from '../src/design/applyRootTheme'

function fakeMatchMedia(matches: boolean): Window['matchMedia'] {
  return (() => ({ matches })) as unknown as Window['matchMedia']
}

describe('theme resolution', () => {
  it('resolves "system" to the system mode and passes explicit modes through', () => {
    expect(resolveThemeMode('system', 'dark')).toBe('dark')
    expect(resolveThemeMode('system', 'light')).toBe('light')
    expect(resolveThemeMode('light', 'dark')).toBe('light')
    expect(resolveThemeMode('dark', 'light')).toBe('dark')
  })

  it('reads the system mode from prefers-color-scheme, defaulting to light', () => {
    expect(getSystemThemeMode(fakeMatchMedia(true))).toBe('dark')
    expect(getSystemThemeMode(fakeMatchMedia(false))).toBe('light')
    // no matchMedia available (e.g. SSR / non-browser) -> light
    expect(getSystemThemeMode(undefined)).toBe('light')
  })
})
