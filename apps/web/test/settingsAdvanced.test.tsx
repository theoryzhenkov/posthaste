import { describe, expect, it } from 'bun:test'
import { fireEvent, render, within } from '@testing-library/react'

import { SettingsAdvanced } from '../src/components/settings-panel/SettingsAdvanced'

import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

describe('SettingsAdvanced', () => {
  it('hides children until the toggle is clicked', () => {
    const view = render(
      <SettingsAdvanced label="Developer">
        <p>inspector toggle</p>
      </SettingsAdvanced>,
    )
    const screen = within(view.container)

    // Trigger is always visible; content is hidden by default.
    const trigger = screen.getByRole('button', { expanded: false })
    expect(trigger.textContent).toContain('Developer')
    expect(screen.queryByText('inspector toggle')).toBeNull()

    // Opening reveals the children + flips aria-expanded.
    fireEvent.click(trigger)
    expect(screen.getByText('inspector toggle')).toBeTruthy()
    expect(
      screen.getByRole('button', { expanded: true }).textContent,
    ).toContain('Developer')

    // Closing hides them again.
    fireEvent.click(screen.getByRole('button', { expanded: true }))
    expect(screen.queryByText('inspector toggle')).toBeNull()
  })

  it('defaults the label to "Advanced"', () => {
    const view = render(
      <SettingsAdvanced>
        <p>hidden</p>
      </SettingsAdvanced>,
    )
    const screen = within(view.container)
    expect(screen.getByRole('button').textContent).toContain('Advanced')
  })
})
