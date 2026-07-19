/**
 * The wall-clock seam (R8, docs/client/L2-charter.md): every "what time is
 * it" read routes through `now`/`nowMs`, so tests fake time by mocking this
 * module (or passing a Date in) instead of reaching for the ambient Date
 * global. The eslint ratchet bans `Date.now()` and bare `new Date()`
 * everywhere else; `new Date(value)` (parsing/deriving) stays legal.
 */

/** The current wall-clock time. */
export function now(): Date {
  return new Date()
}

/** The current wall-clock time in epoch milliseconds — `now()` for callers
 *  doing timestamp arithmetic. */
export function nowMs(): number {
  return Date.now()
}

const MONTHS = [
  'Jan',
  'Feb',
  'Mar',
  'Apr',
  'May',
  'Jun',
  'Jul',
  'Aug',
  'Sep',
  'Oct',
  'Nov',
  'Dec',
] as const

/** Pad a number to 2 digits with a leading zero. */
function pad2(n: number): string {
  return n < 10 ? `0${n}` : String(n)
}

/**
 * Formats a date in MailMate style:
 * - Today:     "Today  HH:MM"
 * - Yesterday: "Yesterday  HH:MM"
 * - Older:     "D Mon YYYY  HH:MM"
 *
 * Uses 24-hour time. A double-space separates the date and time parts
 * for visual breathing room.
 */
export function formatRelativeTime(isoDate: string): string {
  const date = new Date(isoDate)
  const current = now()

  const time = `${pad2(date.getHours())}:${pad2(date.getMinutes())}`

  // Compare calendar dates in local time
  const todayStart = new Date(
    current.getFullYear(),
    current.getMonth(),
    current.getDate(),
  )
  const yesterdayStart = new Date(todayStart.getTime() - 86_400_000)

  if (date >= todayStart) {
    return `Today  ${time}`
  }
  if (date >= yesterdayStart) {
    return `Yesterday  ${time}`
  }

  const day = date.getDate()
  const month = MONTHS[date.getMonth()]
  const year = date.getFullYear()
  return `${day} ${month} ${year}  ${time}`
}
