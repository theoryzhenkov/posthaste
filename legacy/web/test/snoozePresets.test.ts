import { describe, expect, it } from 'bun:test'

import { snoozePresets } from '../src/components/message-detail/snoozePresets'

describe('snoozePresets', () => {
  // A fixed "now": Wednesday 2026-06-24 10:00 local.
  const wednesday = new Date(2026, 5, 24, 10, 0, 0)

  it('includes all four presets before 18:00', () => {
    const labels = snoozePresets(wednesday).map((p) => p.label)
    expect(labels).toEqual([
      'Later today',
      'Tomorrow',
      'This weekend',
      'Next week',
    ])
  })

  it('omits "Later today" once it is past 18:00', () => {
    const evening = new Date(2026, 5, 24, 19, 0, 0)
    const labels = snoozePresets(evening).map((p) => p.label)
    expect(labels).not.toContain('Later today')
    expect(labels).toEqual(['Tomorrow', 'This weekend', 'Next week'])
  })

  it('sets every preset return time in the future', () => {
    const nowUnix = Math.floor(wednesday.getTime() / 1000)
    for (const preset of snoozePresets(wednesday)) {
      expect(preset.until).toBeGreaterThan(nowUnix)
    }
  })

  it('snoozes "Tomorrow" to tomorrow at 09:00 local', () => {
    const tomorrow = snoozePresets(wednesday).find(
      (p) => p.label === 'Tomorrow',
    )!
    const expected = new Date(2026, 5, 25, 9, 0, 0) // Thu 2026-06-25 09:00
    expect(tomorrow.until).toBe(Math.floor(expected.getTime() / 1000))
  })

  it('snoozes "This weekend" to the next Saturday at 09:00 local', () => {
    const weekend = snoozePresets(wednesday).find(
      (p) => p.label === 'This weekend',
    )!
    const expected = new Date(2026, 5, 27, 9, 0, 0) // Sat 2026-06-27 09:00
    expect(weekend.until).toBe(Math.floor(expected.getTime() / 1000))
  })

  it('snoozes "Next week" to the next Monday at 09:00 local', () => {
    const nextWeek = snoozePresets(wednesday).find(
      (p) => p.label === 'Next week',
    )!
    const expected = new Date(2026, 5, 29, 9, 0, 0) // Mon 2026-06-29 09:00
    expect(nextWeek.until).toBe(Math.floor(expected.getTime() / 1000))
  })
})
