/**
 * Pure value helpers for the type-directed condition widgets. Kept separate
 * from the widgets (JSX) file so both stay lint-clean and so the wire-shape
 * parity of each transform can be unit-tested without a DOM.
 *
 * WIRE-SHAPE PARITY (load-bearing): these produce the exact `MailQueryValue`
 * shapes the old text box did — a `string` for single-value ops, a `string[]`
 * for `in`, a `boolean` for booleans — so the compiler/evaluator and stored
 * JSON are unchanged.
 *
 */
import type { DateUnit, DateValue, MailQueryValue } from '../../../api/types'

/** True when a value is the typed absolute-date object `{ kind:'absolute' }`. */
function isAbsoluteDate(
  value: MailQueryValue,
): value is { kind: 'absolute'; value: string } {
  return (
    typeof value === 'object' &&
    value !== null &&
    !Array.isArray(value) &&
    value.kind === 'absolute'
  )
}

/** True when a value is the typed relative-date object `{ kind:'relative' }`. */
function isRelativeDate(
  value: MailQueryValue,
): value is { kind: 'relative'; amount: number; unit: DateUnit } {
  return (
    typeof value === 'object' &&
    value !== null &&
    !Array.isArray(value) &&
    value.kind === 'relative'
  )
}

/**
 * Which date sub-editor a stored value maps to. A `relative` object edits as a
 * rolling offset; everything else (a typed `absolute`, or a legacy bare RFC3339
 * string) edits as an absolute date.
 */
export function dateValueMode(value: MailQueryValue): 'absolute' | 'relative' {
  return isRelativeDate(value) ? 'relative' : 'absolute'
}

/** Extract the `YYYY-MM-DD` a native date input wants from a stored value.
 *  Reads both a legacy bare RFC3339 string and the typed `{kind:'absolute'}`. */
export function dateInputValue(value: MailQueryValue): string {
  const raw =
    typeof value === 'string' ? value : isAbsoluteDate(value) ? value.value : ''
  const match = raw.match(/^(\d{4}-\d{2}-\d{2})/)
  return match ? match[1] : ''
}

/** Turn a native date input's `YYYY-MM-DD` into the RFC3339 string the
 *  evaluator compares `received_at` against (stored as RFC3339 TEXT). */
export function toRfc3339FromDateInput(dateStr: string): string {
  return dateStr ? `${dateStr}T00:00:00Z` : ''
}

/** Build the typed absolute-date value from a native date input string. */
export function absoluteDateValue(dateStr: string): DateValue {
  return { kind: 'absolute', value: toRfc3339FromDateInput(dateStr) }
}

/** The relative units the editor offers (a subset of the wire `DateUnit`). */
export type RelativeUnit = 'days' | 'weeks' | 'months'

// Bare unit labels: the surrounding reading ("in the last N …" / "more than N …
// ago") already supplies the direction, so the unit must NOT read "days ago".
export const RELATIVE_UNIT_OPTIONS: { value: RelativeUnit; label: string }[] = [
  { value: 'days', label: 'days' },
  { value: 'weeks', label: 'weeks' },
  { value: 'months', label: 'months' },
]

/**
 * Build a typed *relative* date value, stored AS-IS. Unlike the old helper,
 * this does NOT resolve the offset to an absolute timestamp at edit time — the
 * `{ kind:'relative', amount, unit }` shape is persisted so the evaluator
 * resolves it against `now` at query time and the window keeps rolling. This is
 * the relative-date freeze bug fix.
 */
export function relativeDateValue(
  amount: number,
  unit: RelativeUnit,
): DateValue {
  const n = Number.isFinite(amount) && amount >= 0 ? Math.trunc(amount) : 0
  return { kind: 'relative', amount: n, unit }
}

/** Best-effort read of a stored relative value into editable amount/unit parts
 *  (defaults to `7 days` for a fresh/non-relative value). */
export function relativeParts(value: MailQueryValue): {
  amount: string
  unit: RelativeUnit
} {
  if (isRelativeDate(value)) {
    const unit = (['days', 'weeks', 'months'] as RelativeUnit[]).includes(
      value.unit as RelativeUnit,
    )
      ? (value.unit as RelativeUnit)
      : 'days'
    return { amount: String(value.amount), unit }
  }
  return { amount: '7', unit: 'days' }
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
export function sizeInputParts(value: MailQueryValue): {
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

// ---------------------------------------------------------------------------
// `in` (list) value helpers — the generic list editor's pure core
// ---------------------------------------------------------------------------

/** Read a stored condition value as the `string[]` the `in` operator holds.
 *  Tolerates a legacy scalar (one non-empty string becomes a one-entry list). */
export function listValueEntries(value: MailQueryValue): string[] {
  if (Array.isArray(value)) {
    return value
  }
  if (typeof value === 'string' && value.trim().length > 0) {
    return [value.trim()]
  }
  return []
}

/**
 * Append a drafted entry to an `in` list. The draft may itself be a
 * comma-separated batch (paste convenience — splits exactly like the old text
 * box); duplicates are dropped so a double-commit is a no-op. Returns the same
 * `string[]` wire shape the old widget emitted.
 */
export function appendListEntries(values: string[], draft: string): string[] {
  const next = [...values]
  for (const entry of splitListValue(draft)) {
    if (!next.includes(entry)) {
      next.push(entry)
    }
  }
  return next
}

/** Remove one entry (by index) from an `in` list. */
export function removeListEntry(values: string[], index: number): string[] {
  return values.filter((_, i) => i !== index)
}
