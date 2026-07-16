import { beforeEach, describe, expect, it } from 'bun:test'
import { act, fireEvent, render, waitFor, within } from '@testing-library/react'

import {
  createClientPreferencesStore,
  type ClientPreferencesStore,
} from '../src/clientPreferences'
import { DesignThemeProvider } from '../src/components/ThemeProvider'
import { useDesignTheme } from '../src/hooks/useDesignTheme'
import { designDataAttributes, designStorageKeys } from '../src/design'
import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

function ThemeProbe() {
  const theme = useDesignTheme()
  return <span data-testid="theme-id">{theme.theme}</span>
}

function ThemePaletteButton() {
  const theme = useDesignTheme()
  return (
    <button type="button" onClick={() => theme.setTheme('glass')}>
      Use glass
    </button>
  )
}

// A fresh store per test isolates this suite from the process-wide singleton's
// cross-window-sync state, so the synthetic StorageEvent below is not coupled to
// the global subscribe ordering of the whole run.
let store: ClientPreferencesStore

beforeEach(() => {
  window.localStorage.clear()
  document.documentElement.removeAttribute(designDataAttributes.palettePreset)
  store = createClientPreferencesStore()
})

describe('DesignThemeProvider', () => {
  it('persists appearance changes through the client preferences store', async () => {
    const view = render(
      <DesignThemeProvider store={store}>
        <ThemeProbe />
        <ThemePaletteButton />
      </DesignThemeProvider>,
    )

    const screen = within(view.container)

    fireEvent.click(screen.getByRole('button', { name: 'Use glass' }))

    await waitFor(() =>
      expect(screen.getByTestId('theme-id').textContent).toBe('glass'),
    )
    expect(window.localStorage.getItem(designStorageKeys.theme)).toBe('glass')
  })

  it('applies appearance changes written by another window', async () => {
    const view = render(
      <DesignThemeProvider store={store}>
        <ThemeProbe />
      </DesignThemeProvider>,
    )

    const screen = within(view.container)

    expect(screen.getByTestId('theme-id').textContent).toBe('neutral')
    await waitFor(() =>
      expect(
        document.documentElement.getAttribute(
          designDataAttributes.palettePreset,
        ),
      ).toBe('neutral'),
    )

    act(() => {
      window.localStorage.setItem(designStorageKeys.theme, 'glass')
      window.dispatchEvent(
        new StorageEvent('storage', {
          key: designStorageKeys.theme,
          newValue: 'glass',
          oldValue: 'neutral',
          storageArea: window.localStorage,
        }),
      )
    })

    await waitFor(() =>
      expect(screen.getByTestId('theme-id').textContent).toBe('glass'),
    )
    expect(
      document.documentElement.getAttribute(designDataAttributes.palettePreset),
    ).toBe('glass')
  })
})
