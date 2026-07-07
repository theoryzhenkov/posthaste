/**
 * Send-later preset times for the composer's "Send later" menu — the snooze
 * presets' pattern applied to outgoing mail. Each preset carries an RFC 3339
 * `sendAt` the send command is scheduled with (the backend holds the outbox
 * send until due; local-first — it fires when Posthaste is next running and
 * online at/after that time).
 */

export interface SendLaterPreset {
  label: string
  /** RFC 3339 scheduled time. */
  sendAt: string
  /** Preformatted human time shown next to the label. */
  hint: string
}

function atHour(date: Date, hour: number): Date {
  const result = new Date(date)
  result.setHours(hour, 0, 0, 0)
  return result
}

function addDays(date: Date, days: number): Date {
  const result = new Date(date)
  result.setDate(result.getDate() + days)
  return result
}

function nextWeekdayMorning(now: Date, weekday: number): Date {
  const result = atHour(now, 9)
  do {
    result.setDate(result.getDate() + 1)
  } while (result.getDay() !== weekday)
  return result
}

const HINT_FORMAT: Intl.DateTimeFormatOptions = {
  weekday: 'short',
  hour: 'numeric',
  minute: '2-digit',
}

function preset(label: string, at: Date): SendLaterPreset {
  return {
    label,
    sendAt: at.toISOString(),
    hint: at.toLocaleString(undefined, HINT_FORMAT),
  }
}

export function sendLaterPresets(now: Date = new Date()): SendLaterPreset[] {
  const presets: SendLaterPreset[] = []
  const tonight = atHour(now, 18)
  if (tonight > now) {
    presets.push(preset('Tonight', tonight))
  }
  presets.push(preset('Tomorrow morning', atHour(addDays(now, 1), 9)))
  presets.push(preset('Monday morning', nextWeekdayMorning(now, 1)))
  return presets
}

/**
 * Format a scheduled time for toast/outbox copy (e.g. "Tue 9:00 AM").
 * Falls back to the raw string if unparseable.
 */
export function formatScheduledTime(sendAt: string): string {
  const parsed = new Date(sendAt)
  if (Number.isNaN(parsed.getTime())) {
    return sendAt
  }
  const sameYear = parsed.getFullYear() === new Date().getFullYear()
  return parsed.toLocaleString(undefined, {
    month: 'short',
    day: 'numeric',
    hour: 'numeric',
    minute: '2-digit',
    ...(sameYear ? {} : { year: 'numeric' }),
  })
}
