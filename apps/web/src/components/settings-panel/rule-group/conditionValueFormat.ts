/**
 * Pure value helpers for the type-directed condition widgets. Kept separate
 * from the widgets (JSX) file so both stay lint-clean and so the wire-shape
 * parity of each transform can be unit-tested without a DOM.
 *
 * WIRE-SHAPE PARITY (load-bearing): these produce the exact `SmartMailboxValue`
 * shapes the old text box did — a `string` for single-value ops, a `string[]`
 * for `in`, a `boolean` for booleans — so the compiler/evaluator and stored
 * JSON are unchanged.
 *
 * @spec docs/L1-search#smart-mailbox-data-model
 */
import type { SmartMailboxValue } from '../../../api/types'

/** Extract the `YYYY-MM-DD` a native date input wants from a stored value. */
export function dateInputValue(value: SmartMailboxValue): string {
  if (typeof value !== 'string') return ''
  const match = value.match(/^(\d{4}-\d{2}-\d{2})/)
  return match ? match[1] : ''
}

/** Turn a native date input's `YYYY-MM-DD` into the RFC3339 string the
 *  evaluator compares `received_at` against (stored as RFC3339 TEXT). */
export function toRfc3339FromDateInput(dateStr: string): string {
  return dateStr ? `${dateStr}T00:00:00Z` : ''
}

export type RelativeUnit = 'days' | 'weeks' | 'months'

export const RELATIVE_UNIT_OPTIONS: { value: RelativeUnit; label: string }[] = [
  { value: 'days', label: 'days ago' },
  { value: 'weeks', label: 'weeks ago' },
  { value: 'months', label: 'months ago' },
]

/**
 * Resolve a relative "N units ago" selection to an absolute RFC3339 string.
 * The smart-mailbox evaluator compares stored values literally, so relative
 * input is resolved to an absolute timestamp at edit time.
 */
export function relativeDateValue(
  amount: number,
  unit: RelativeUnit,
  now: Date,
): string {
  const shifted = new Date(now.getTime())
  const n = Number.isFinite(amount) ? amount : 0
  if (unit === 'weeks') {
    shifted.setUTCDate(shifted.getUTCDate() - n * 7)
  } else if (unit === 'months') {
    shifted.setUTCMonth(shifted.getUTCMonth() - n)
  } else {
    shifted.setUTCDate(shifted.getUTCDate() - n)
  }
  return shifted.toISOString().replace(/\.\d{3}Z$/, 'Z')
}

/** Sentinel a picker uses for "nothing chosen"; maps back to the empty string. */
export const UNSET_REF = '__unset__'

/**
 * Map a ref picker's raw selection (mailbox/account/role id, or the unset
 * sentinel) to the emitted value — always a plain `string`, matching the wire
 * shape the old text box produced for these id fields.
 */
export function pickedRefValue(raw: string): string {
  return raw === UNSET_REF ? '' : raw
}

// ---------------------------------------------------------------------------
// Size (bytes + unit) helpers
// ---------------------------------------------------------------------------

/** Byte-size unit the size widget offers. */
export type SizeUnit = 'bytes' | 'kb' | 'mb'

export const SIZE_UNIT_OPTIONS: { value: SizeUnit; label: string }[] = [
  { value: 'bytes', label: 'bytes' },
  { value: 'kb', label: 'KB' },
  { value: 'mb', label: 'MB' },
]

/** Multiplier to convert a value in `unit` into bytes (KB/MB are binary: 1024). */
const SIZE_UNIT_BYTES: Record<SizeUnit, number> = {
  bytes: 1,
  kb: 1024,
  mb: 1024 * 1024,
}

/**
 * Convert an `amount` in `unit` to the byte-count STRING the compiler expects.
 * The `size` field compiles to a numeric comparison on `message.size` (bytes),
 * so the wire value is always bytes encoded as a string — the same string wire
 * shape every other single-value operator uses. A blank/invalid amount emits
 * the empty string (an unset condition), never `NaN`.
 */
export function bytesFromSize(amount: number, unit: SizeUnit): string {
  if (!Number.isFinite(amount) || amount < 0) return ''
  return String(Math.round(amount * SIZE_UNIT_BYTES[unit]))
}

/**
 * Best-effort reverse of {@link bytesFromSize}: pick the largest unit that
 * represents the stored byte count without a fractional remainder, so an edited
 * condition round-trips to a friendly `amount`/`unit` pair. Falls back to bytes.
 */
export function sizeInputParts(value: SmartMailboxValue): {
  amount: string
  unit: SizeUnit
} {
  if (typeof value !== 'string' || value.trim() === '') {
    return { amount: '', unit: 'kb' }
  }
  const bytes = Number(value)
  if (!Number.isFinite(bytes)) return { amount: '', unit: 'kb' }
  for (const unit of ['mb', 'kb'] as SizeUnit[]) {
    const factor = SIZE_UNIT_BYTES[unit]
    if (bytes >= factor && bytes % factor === 0) {
      return { amount: String(bytes / factor), unit }
    }
  }
  return { amount: String(bytes), unit: 'bytes' }
}

/** Split the comma-separated `in` text box into a `string[]` (unchanged). */
export function splitListValue(text: string): string[] {
  return text
    .split(',')
    .map((entry) => entry.trim())
    .filter(Boolean)
}
