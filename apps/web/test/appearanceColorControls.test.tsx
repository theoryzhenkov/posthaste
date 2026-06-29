import { describe, expect, it, mock } from 'bun:test'
import { fireEvent, render, within } from '@testing-library/react'

import { ColorsSection } from '../src/components/settings-panel/appearance/ThemeSections'
import type { useDesignTheme } from '../src/hooks/useDesignTheme'
import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

type DesignTheme = ReturnType<typeof useDesignTheme>

function makeTheme(overrides: Partial<DesignTheme> = {}): DesignTheme {
  return {
    theme: 'neutral',
    mode: 'dark',
    resolvedMode: 'dark',
    density: 'compact',
    light: { accentHue: 45, surfaceHue: 60 },
    dark: { accentHue: 45, surfaceHue: 60 },
    glassTheme: { blooms: [] },
    setAccentHue: mock(() => {}),
    setSurfaceHue: mock(() => {}),
    setTheme: mock(() => {}),
    setMode: mock(() => {}),
    setDensity: mock(() => {}),
    addGlassBloom: mock(() => 'b'),
    removeGlassBloom: mock(() => {}),
    setGlassBloom: mock(() => {}),
    ...overrides,
  } as unknown as DesignTheme
}

describe('appearance color controls', () => {
  it('shows accent + surface controls for both modes on a solid theme', () => {
    const theme = makeTheme()
    const view = render(<ColorsSection theme={theme} />)
    const screen = within(view.container)
    // light + dark each have an Accent + a Surface hue control.
    expect(screen.getAllByLabelText('Accent hue')).toHaveLength(2)
    expect(screen.getAllByLabelText('Surface hue')).toHaveLength(2)
  })

  it('hides the surface control for the glass theme (mesh-driven)', () => {
    const theme = makeTheme({ theme: 'glass' })
    const view = render(<ColorsSection theme={theme} />)
    const screen = within(view.container)
    expect(screen.getAllByLabelText('Accent hue')).toHaveLength(2)
    expect(screen.queryAllByLabelText('Surface hue')).toHaveLength(0)
  })

  it('commits a typed hue on blur (per-mode), not per keystroke', () => {
    const theme = makeTheme()
    const view = render(<ColorsSection theme={theme} />)
    const screen = within(view.container)
    const lightAccent = screen.getAllByLabelText('Accent hue')[0]

    fireEvent.change(lightAccent, { target: { value: '200' } })
    // Draft only — no commit yet (avoids a write-through PATCH per keystroke).
    expect(theme.setAccentHue).not.toHaveBeenCalled()

    fireEvent.blur(lightAccent)
    expect(theme.setAccentHue).toHaveBeenCalledTimes(1)
    expect(theme.setAccentHue).toHaveBeenCalledWith('light', 200)
  })

  it('commits a typed hue on Enter', () => {
    const theme = makeTheme()
    const view = render(<ColorsSection theme={theme} />)
    const screen = within(view.container)
    const darkAccent = screen.getAllByLabelText('Accent hue')[1]

    fireEvent.change(darkAccent, { target: { value: '300' } })
    // Enter commits + blurs; the blur is what fires onBlur in jsdom.
    fireEvent.keyDown(darkAccent, { key: 'Enter' })
    fireEvent.blur(darkAccent)
    expect(theme.setAccentHue).toHaveBeenCalledWith('dark', 300)
  })

  it('strips non-digits from the typed hue', () => {
    const theme = makeTheme()
    const view = render(<ColorsSection theme={theme} />)
    const screen = within(view.container)
    const lightSurface = screen.getAllByLabelText('Surface hue')[0]

    fireEvent.change(lightSurface, { target: { value: '1a2b' } })
    fireEvent.blur(lightSurface)
    expect(theme.setSurfaceHue).toHaveBeenCalledWith('light', 12)
  })

  it('resets a hue to its default via the reset button', () => {
    const theme = makeTheme({ dark: { accentHue: 300, surfaceHue: 60 } })
    const view = render(<ColorsSection theme={theme} />)
    const screen = within(view.container)
    // Dark accent is customized (300) → its reset is enabled.
    const reset = screen.getAllByLabelText('Reset Accent to default')[1]
    expect((reset as HTMLButtonElement).disabled).toBe(false)
    fireEvent.click(reset)
    expect(theme.setAccentHue).toHaveBeenCalledWith('dark', 45)
  })
})
