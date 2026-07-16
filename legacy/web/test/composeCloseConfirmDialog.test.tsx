import { describe, expect, it } from 'bun:test'
import { fireEvent, render } from '@testing-library/react'

import { ComposeCloseConfirmDialog } from '../src/components/compose-overlay/ComposeCloseConfirmDialog'

import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

function renderDialog(
  handlers: Partial<{
    onKeepEditing: () => void
    onDiscard: () => void
    onSaveAsDraft: () => void
  }> = {},
  intentKind: 'new' | 'draft' = 'new',
) {
  const noop = () => {}
  return render(
    <ComposeCloseConfirmDialog
      open
      intentKind={intentKind}
      onKeepEditing={handlers.onKeepEditing ?? noop}
      onDiscard={handlers.onDiscard ?? noop}
      onSaveAsDraft={handlers.onSaveAsDraft ?? noop}
    />,
  )
}

describe('ComposeCloseConfirmDialog', () => {
  it('shows the three actions when a dirty compose is closed', () => {
    const { getByRole } = renderDialog()
    expect(getByRole('button', { name: 'Save as draft' })).toBeDefined()
    expect(getByRole('button', { name: 'Discard' })).toBeDefined()
    expect(getByRole('button', { name: 'Keep editing' })).toBeDefined()
  })

  it('Save as draft routes to the save handler', () => {
    let saved = 0
    const { getByRole } = renderDialog({ onSaveAsDraft: () => (saved += 1) })
    fireEvent.click(getByRole('button', { name: 'Save as draft' }))
    expect(saved).toBe(1)
  })

  it('Discard routes to the discard handler (close without saving)', () => {
    let discarded = 0
    const { getByRole } = renderDialog({ onDiscard: () => (discarded += 1) })
    fireEvent.click(getByRole('button', { name: 'Discard' }))
    expect(discarded).toBe(1)
  })

  it('Keep editing cancels the close', () => {
    let kept = 0
    const { getByRole } = renderDialog({ onKeepEditing: () => (kept += 1) })
    fireEvent.click(getByRole('button', { name: 'Keep editing' }))
    expect(kept).toBe(1)
  })

  it('words the title for a resumed draft', () => {
    const { getByText } = renderDialog({}, 'draft')
    expect(getByText('Save changes to this draft?')).toBeDefined()
  })
})
