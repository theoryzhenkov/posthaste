/**
 * Calendar-date parsing (the R3 boundary for `date:` search values and any
 * other `yyyy-mm-dd` input) and the snooze-preset clock math.
 */

/** A validated `yyyy-mm-dd` calendar date (a REAL date, not just the shape). */
export type IsoDate = string & { readonly __brand: 'IsoDate' }

/** Parse raw text into a calendar date: `yyyy-mm-dd` shape AND a real date
 *  (round-trips through `Date`, so `2026-02-30` is rejected). */
export function parseIsoDate(raw: string): IsoDate | null {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(raw)) {
    return null
  }
  const date = new Date(`${raw}T00:00:00.000Z`)
  return !Number.isNaN(date.getTime()) && date.toISOString().startsWith(raw)
    ? (raw as IsoDate)
    : null
}

/** The calendar date of `now`, in UTC (feeds `date:` completions). */
export function todayIsoDate(now: Date): IsoDate {
  return now.toISOString().slice(0, 10) as IsoDate
}

/** Snooze preset return times (unix seconds, local). The scheduler
 * (`MailService::auto_return_snoozed_messages`) moves a snoozed message back
 * to the Inbox when `until <= now`. Options source for the parameterized
 * `message.snooze` action. */

export type SnoozePreset = { label: string; until: number }

function atHour(date: Date, hour: number): Date {
  const d = new Date(date)
  d.setHours(hour, 0, 0, 0)
  return d
}

function addDays(date: Date, days: number): Date {
  const d = new Date(date)
  d.setDate(d.getDate() + days)
  return d
}

function toUnix(date: Date): number {
  return Math.floor(date.getTime() / 1000)
}

/** The next `weekday` (0=Sun..6=Sat) at 09:00 local — this week's if it's still
 * ahead, else next week's. */
function nextWeekdayMorning(now: Date, weekday: number): Date {
  const todayMorning = atHour(now, 9)
  let daysUntil = (weekday - now.getDay() + 7) % 7
  if (daysUntil === 0 && todayMorning <= now) {
    daysUntil = 7
  }
  return atHour(addDays(now, daysUntil), 9)
}

/** The snooze presets shown in the message-header popover. "Later today" is
 * omitted once it's past 18:00 (there's no meaningful "later" left today). */
export function snoozePresets(now: Date = new Date()): SnoozePreset[] {
  const presets: SnoozePreset[] = []
  const laterToday = atHour(now, 18)
  if (laterToday > now) {
    presets.push({ label: 'Later today', until: toUnix(laterToday) })
  }
  presets.push({ label: 'Tomorrow', until: toUnix(atHour(addDays(now, 1), 9)) })
  presets.push({
    label: 'This weekend',
    until: toUnix(nextWeekdayMorning(now, 6)),
  })
  presets.push({
    label: 'Next week',
    until: toUnix(nextWeekdayMorning(now, 1)),
  })
  return presets
}
