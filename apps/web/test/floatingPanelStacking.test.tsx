import { describe, expect, it, beforeEach } from 'bun:test'
import { fireEvent, render } from '@testing-library/react'

import { FloatingPanel } from '../src/components/FloatingPanel'
import { ComposeCloseConfirmDialog } from '../src/components/compose-overlay/ComposeCloseConfirmDialog'
import {
  resetWindowStacking,
  WINDOW_BAND_MAX,
  WINDOW_BAND_MIN,
  Z,
} from '../src/layering'

import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

function Panel({
  bodyId,
  layer,
}: {
  bodyId: string
  layer?: 'window' | 'overlay'
}) {
  return (
    <FloatingPanel
      layer={layer}
      panelLabel={bodyId}
      storageKey={`test.${bodyId}`}
      header={<div>header</div>}
      onClose={() => {}}
    >
      <div data-testid={bodyId}>body</div>
    </FloatingPanel>
  )
}

function zOf(body: HTMLElement): number {
  const outer = body.closest('[aria-live="polite"]') as HTMLElement
  return Number.parseInt(outer.style.zIndex, 10)
}

describe('FloatingPanel layering', () => {
  beforeEach(() => resetWindowStacking())

  it('renders the command palette (overlay) above a compose window', () => {
    const compose = render(<Panel bodyId="compose" layer="window" />)
    const palette = render(<Panel bodyId="palette" layer="overlay" />)

    const composeZ = zOf(compose.getByTestId('compose'))
    const paletteZ = zOf(palette.getByTestId('palette'))

    expect(paletteZ).toBe(Z.OVERLAY)
    expect(composeZ).toBeGreaterThanOrEqual(WINDOW_BAND_MIN)
    expect(composeZ).toBeLessThanOrEqual(WINDOW_BAND_MAX)
    expect(paletteZ).toBeGreaterThan(composeZ)
  })

  it('brings a focused window to the front of its peers but keeps it below overlay', () => {
    const { getByTestId } = render(
      <>
        <Panel bodyId="first" layer="window" />
        <Panel bodyId="second" layer="window" />
      </>,
    )

    // Newest-opened starts on top.
    expect(zOf(getByTestId('second'))).toBeGreaterThan(
      zOf(getByTestId('first')),
    )

    // Clicking the older window raises it above the newer one.
    fireEvent.pointerDown(getByTestId('first'))
    expect(zOf(getByTestId('first'))).toBeGreaterThan(
      zOf(getByTestId('second')),
    )

    // ...but a raised window never crosses into the OVERLAY tier.
    expect(zOf(getByTestId('first'))).toBeLessThan(Z.OVERLAY)
  })
})

describe('discard-draft confirmation dialog layering', () => {
  it('renders on the MODAL tier so it sits above compose windows', () => {
    const { getByRole } = render(
      <ComposeCloseConfirmDialog
        open
        intentKind="new"
        onKeepEditing={() => {}}
        onDiscard={() => {}}
        onSaveAsDraft={() => {}}
      />,
    )
    const content = getByRole('alertdialog')
    // MODAL tier token, which the unit scale proves is > the WINDOW band.
    expect(content.className).toContain('z-(--z-modal)')
  })
})
