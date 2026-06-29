import { afterEach, describe, expect, it, spyOn } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'

import * as apiClient from '../src/api/client'
import type { AppSettings, Appearance } from '../src/api/types'
import { createClientPreferencesStore } from '../src/clientPreferences'
import { designToWireAppearance } from '../src/client-preferences/wireMapping'
import { useAppearanceSettingsSync } from '../src/hooks/useAppearanceSettingsSync'
import { defaultThemePreferences } from '../src/themeSettings'
import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

const DEFAULT_SETTINGS: AppSettings = {
  defaultAccountId: null,
  cachePolicy: {
    softCapBytes: 1,
    hardCapBytes: 2,
    cacheBodies: true,
    cacheRawMessages: false,
    cacheAttachments: true,
  },
  automationRules: [],
  automationDrafts: [],
  appearance: null,
}

function makeWrapper(qc: QueryClient) {
  return ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={qc}>{children}</QueryClientProvider>
  )
}

// spyOn does not auto-restore between tests in the same file, so track + restore
// explicitly to keep call counts isolated.
const spies: Array<ReturnType<typeof spyOn>> = []

afterEach(() => {
  while (spies.length) {
    spies.pop()?.mockRestore()
  }
  window.localStorage.clear()
})

function mockFetchSettings(settings: AppSettings) {
  spies.push(
    spyOn(apiClient, 'fetchSettings').mockReturnValue(
      Promise.resolve(settings),
    ),
  )
}

function mockPatchSettings(resolved: AppSettings) {
  const spy = spyOn(apiClient, 'patchSettings').mockReturnValue(
    Promise.resolve(resolved),
  )
  spies.push(spy)
  return spy
}

describe('useAppearanceSettingsSync', () => {
  it('reconciles TOML -> cache when TOML differs (TOML wins)', async () => {
    const store = createClientPreferencesStore()
    // Cache starts at the renderer default (mode 'dark').
    expect(store.getSnapshot().appearance.mode).toBe(
      defaultThemePreferences().mode,
    )

    const tomlAppearance: Appearance = {
      mode: 'light',
      theme: 'glass',
      density: 'comfortable',
      light: { accentHue: 200 },
      glassTheme: null,
    }
    mockFetchSettings({ ...DEFAULT_SETTINGS, appearance: tomlAppearance })

    const qc = new QueryClient()
    renderHook(() => useAppearanceSettingsSync(store), {
      wrapper: makeWrapper(qc),
    })

    await waitFor(() => {
      expect(store.getSnapshot().appearance.mode).toBe('light')
    })
    expect(store.getSnapshot().appearance.theme).toBe('glass')
    expect(store.getSnapshot().appearance.light.accentHue).toBe(200)
  })

  it('imports a non-default cache to TOML once when TOML is unset', async () => {
    const store = createClientPreferencesStore()
    // Seed a non-default cache (so import is not a no-op).
    const base = defaultThemePreferences()
    const customized = {
      ...base,
      light: { ...base.light, accentHue: 210 },
    }
    store.setAppearance(customized)

    mockFetchSettings(DEFAULT_SETTINGS)
    const importSpy = mockPatchSettings({
      ...DEFAULT_SETTINGS,
      appearance: designToWireAppearance(customized),
    })

    const qc = new QueryClient()
    renderHook(() => useAppearanceSettingsSync(store), {
      wrapper: makeWrapper(qc),
    })

    await waitFor(() => {
      expect(importSpy).toHaveBeenCalledTimes(1)
    })
    const patchArg = importSpy.mock.calls[0][0]
    expect(patchArg.appearance?.light?.accentHue).toBe(210)
    expect(window.localStorage.getItem('posthaste.appearance.imported')).toBe(
      '1',
    )
  })

  it('does not import when the cache is the renderer default', async () => {
    const store = createClientPreferencesStore() // default cache
    mockFetchSettings(DEFAULT_SETTINGS)
    const importSpy = mockPatchSettings(DEFAULT_SETTINGS)

    const qc = new QueryClient()
    renderHook(() => useAppearanceSettingsSync(store), {
      wrapper: makeWrapper(qc),
    })

    // Give the effect a chance to run; the cache stays default, so no import.
    await waitFor(() =>
      expect(store.getSnapshot().appearance).toEqual(defaultThemePreferences()),
    )
    expect(importSpy).not.toHaveBeenCalled()
  })
})
