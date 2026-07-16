import { describe, expect, it } from 'bun:test'

import { computeCardPosition } from '../src/onboarding/position'

const viewport = { width: 1000, height: 800 }
const card = { width: 340, height: 200 }

describe('onboarding card placement', () => {
  it('centers the card when there is no anchor', () => {
    expect(computeCardPosition({ anchor: null, viewport, card })).toEqual({
      top: (800 - 200) / 2,
      left: (1000 - 340) / 2,
    })
  })

  it('places the card below an anchor with room beneath it', () => {
    const anchor = { top: 100, left: 200, width: 120, height: 40 }
    const pos = computeCardPosition({ anchor, viewport, card })
    expect(pos.top).toBe(100 + 40 + 12) // below + gap
    expect(pos.left).toBe(200) // aligned to anchor's left edge
  })

  it('flips above the anchor when there is no room below', () => {
    const anchor = { top: 700, left: 50, width: 100, height: 40 }
    const pos = computeCardPosition({ anchor, viewport, card })
    expect(pos.top).toBe(700 - 12 - 200) // above + gap
  })

  it('clamps the card within the viewport for an anchor near the right edge', () => {
    const anchor = { top: 20, left: 980, width: 16, height: 16 }
    const pos = computeCardPosition({ anchor, viewport, card })
    // left would overflow (980 + 340 > 1000); clamp to viewport - card - margin
    expect(pos.left).toBe(1000 - 340 - 12)
    expect(pos.left).toBeGreaterThanOrEqual(12)
  })
})
