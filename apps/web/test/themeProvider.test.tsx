import { beforeEach, describe, expect, it } from 'bun:test'
import { act, fireEvent, render, waitFor } from '@testing-library/react'

import { clientPreferencesStore } from '../src/clientPreferences'
import { DesignThemeProvider } from '../src/components/ThemeProvider'
import { useDesignTheme } from '../src/hooks/useDesignTheme'
import { designDataAttributes, designStorageKeys } from '../src/design'
import { defaultThemePreferences } from '../src/themeSettings'
import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

function ThemeProbe() {
  const theme = useDesignTheme()
  return <span data-testid="palette-preset">{theme.palettePreset}</span>
}

function ThemePaletteButton() {
  const theme = useDesignTheme()
  return (
    <button type="button" onClick={() => theme.setPalettePreset('glass')}>
      Use glass
    </button>
  )
}

beforeEach(() => {
  window.localStorage.clear()
  clientPreferencesStore.setAppearance(defaultThemePreferences())
  document.documentElement.removeAttribute(designDataAttributes.palettePreset)
})

describe('DesignThemeProvider', () => {
  it('persists appearance changes through the client preferences store', async () => {
    const view = render(
      <DesignThemeProvider>
        <ThemeProbe />
        <ThemePaletteButton />
      </DesignThemeProvider>,
    )

    fireEvent.click(view.getByRole('button', { name: 'Use glass' }))

    await waitFor(() =>
      expect(view.getByTestId('palette-preset').textContent).toBe('glass'),
    )
    expect(window.localStorage.getItem(designStorageKeys.palettePreset)).toBe(
      'glass',
    )
  })

  it('applies appearance changes written by another window', async () => {
    const view = render(
      <DesignThemeProvider>
        <ThemeProbe />
      </DesignThemeProvider>,
    )

    expect(view.getByTestId('palette-preset').textContent).toBe('neutral')

    act(() => {
      window.localStorage.setItem(designStorageKeys.palettePreset, 'glass')
      window.dispatchEvent(
        new StorageEvent('storage', {
          key: designStorageKeys.palettePreset,
          newValue: 'glass',
          oldValue: 'neutral',
          storageArea: window.localStorage,
        }),
      )
    })

    await waitFor(() =>
      expect(view.getByTestId('palette-preset').textContent).toBe('glass'),
    )
    expect(
      document.documentElement.getAttribute(designDataAttributes.palettePreset),
    ).toBe('glass')
  })
})
