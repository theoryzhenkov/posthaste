import { describe, expect, it } from 'bun:test'

import {
  formatScheduledTime,
  sendLaterPresets,
} from '../src/components/compose-overlay/sendLaterPresets'

describe('sendLaterPresets', () => {
  // A Wednesday morning.
  const wednesdayMorning = new Date(2026, 5, 24, 10, 0, 0)
  // A Wednesday evening, past the "Tonight" cutoff.
  const wednesdayEvening = new Date(2026, 5, 24, 19, 30, 0)

  it('offers Tonight before 18:00 and drops it after', () => {
    expect(sendLaterPresets(wednesdayMorning).map((p) => p.label)).toEqual([
      'Tonight',
      'Tomorrow morning',
      'Monday morning',
    ])
    expect(sendLaterPresets(wednesdayEvening).map((p) => p.label)).toEqual([
      'Tomorrow morning',
      'Monday morning',
    ])
  })

  it('every preset is a parseable RFC 3339 instant strictly in the future', () => {
    for (const now of [wednesdayMorning, wednesdayEvening]) {
      for (const preset of sendLaterPresets(now)) {
        const parsed = new Date(preset.sendAt)
        expect(Number.isNaN(parsed.getTime())).toBe(false)
        expect(parsed.getTime()).toBeGreaterThan(now.getTime())
      }
    }
  })

  it('schedules mornings at 09:00 local and Monday on the next Monday', () => {
    const presets = sendLaterPresets(wednesdayMorning)
    const tomorrow = new Date(
      presets.find((p) => p.label === 'Tomorrow morning')!.sendAt,
    )
    expect(tomorrow.getHours()).toBe(9)
    expect(tomorrow.getDate()).toBe(25)
    const monday = new Date(
      presets.find((p) => p.label === 'Monday morning')!.sendAt,
    )
    expect(monday.getDay()).toBe(1)
    expect(monday.getHours()).toBe(9)
  })

  it('formatScheduledTime falls back to the raw string when unparseable', () => {
    expect(formatScheduledTime('not-a-time')).toBe('not-a-time')
    expect(formatScheduledTime(wednesdayMorning.toISOString())).toContain('24')
  })
})
