/** Snooze preset return times (unix seconds, local). The scheduler
 * (`MailService::auto_return_snoozed_messages`) moves a snoozed message back
 * to the Inbox when `until <= now`. */

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
