import { describe, expect, it } from 'bun:test'

import { shouldCloseOriginalComposeAfterWindowOpen } from '../src/composeWindowElevation'

describe('compose window elevation draft-loss guard', () => {
  it('closes the original compose when the draft stayed pristine', () => {
    expect(
      shouldCloseOriginalComposeAfterWindowOpen({
        openingResetKey: 'new:primary',
        lastEditedResetKey: null,
      }),
    ).toBe(true)
  })

  it('keeps the original compose when the active draft changed while opening', () => {
    expect(
      shouldCloseOriginalComposeAfterWindowOpen({
        openingResetKey: 'new:primary',
        lastEditedResetKey: 'new:primary',
      }),
    ).toBe(false)
  })

  it('does not treat stale edits from an earlier reset as active draft edits', () => {
    expect(
      shouldCloseOriginalComposeAfterWindowOpen({
        openingResetKey: 'reply:primary:m1:ready',
        lastEditedResetKey: 'reply:primary:m1:loading',
      }),
    ).toBe(true)
  })
})
