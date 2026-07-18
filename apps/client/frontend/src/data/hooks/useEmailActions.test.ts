import { describe, expect, test } from 'bun:test'

import { undoToastOptions } from './useEmailActions'

// Regression for docs/issues/integrated-send-undo-broken.md: the action
// toast's Undo must issue the undo verb against THE ACCOUNT THE ACTION RAN
// ON. It used to call the shell's default-account undo binding, which
// silently no-oped (or undid something else) for every other account.
describe('undoToastOptions', () => {
  test('binds Undo to the acted-on account', () => {
    const undone: string[] = []
    const options = undoToastOptions('acct-2', (sourceId) =>
      undone.push(sourceId),
    )
    expect(options.action?.label).toBe('Undo')
    options.action?.onClick()
    expect(undone).toEqual(['acct-2'])
  })

  test('an irreversible action gets no Undo', () => {
    const options = undoToastOptions(undefined, () => {
      throw new Error('must not be called')
    })
    expect(options.action).toBeUndefined()
    expect(options.duration).toBe(5000)
  })
})
