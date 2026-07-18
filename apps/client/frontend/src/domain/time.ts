/**
 * Calendar-date parsing (the R3 boundary for `date:` search values and any
 * other `yyyy-mm-dd` input).
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
