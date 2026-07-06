import { describe, expect, it } from 'bun:test'

import type { SmartMailboxCondition, SmartMailboxField } from '../src/api/types'
import {
  FIELD_REGISTRY,
  operatorOptionsForField,
  valueTypeForField,
} from '../src/components/settings-panel/helpers/fieldRegistry'
import {
  bytesFromSize,
  dateInputValue,
  pickedRefValue,
  relativeDateValue,
  sizeInputParts,
  splitListValue,
  toRfc3339FromDateInput,
} from '../src/components/settings-panel/rule-group/conditionValueFormat'

describe('fieldRegistry', () => {
  it('maps every field to a value type (mirrors the Rust compiler matrix)', () => {
    expect(valueTypeForField('receivedAt')).toBe('date')
    expect(valueTypeForField('mailboxId')).toBe('mailboxRef')
    expect(valueTypeForField('sourceId')).toBe('accountRef')
    expect(valueTypeForField('mailboxRole')).toBe('roleEnum')
    expect(valueTypeForField('isRead')).toBe('boolean')
    expect(valueTypeForField('isFlagged')).toBe('boolean')
    expect(valueTypeForField('hasAttachment')).toBe('boolean')
    expect(valueTypeForField('subject')).toBe('text')
    expect(valueTypeForField('to')).toBe('address')
    expect(valueTypeForField('size')).toBe('size')
  })

  it('preserves the exact operator subsets the old switch returned', () => {
    // Wire-compat: the operator matrix must not drift when moved into the
    // registry, or existing rules would offer different operators.
    const expected: Record<SmartMailboxField, string[]> = {
      sourceId: ['equals', 'in'],
      sourceName: ['equals', 'contains', 'in'],
      messageId: ['equals', 'in'],
      threadId: ['equals', 'in'],
      mailboxId: ['equals', 'in'],
      mailboxName: ['equals', 'contains', 'in'],
      mailboxRole: ['equals', 'in'],
      isRead: ['equals'],
      isFlagged: ['equals'],
      hasAttachment: ['equals'],
      keyword: ['equals', 'in'],
      fromName: ['equals', 'contains', 'in'],
      fromEmail: ['equals', 'contains', 'in'],
      to: ['equals', 'contains', 'in'],
      subject: ['equals', 'contains', 'in'],
      preview: ['equals', 'contains', 'in'],
      receivedAt: ['before', 'after', 'onOrBefore', 'onOrAfter'],
      size: ['before', 'after', 'onOrBefore', 'onOrAfter'],
    }
    for (const field of Object.keys(expected) as SmartMailboxField[]) {
      expect(operatorOptionsForField(field)).toEqual(expected[field])
    }
  })

  it('covers exactly the known fields', () => {
    expect(Object.keys(FIELD_REGISTRY).sort()).toHaveLength(18)
  })
})

describe('date value helpers', () => {
  it('extracts YYYY-MM-DD from a stored RFC3339 value', () => {
    expect(dateInputValue('2026-07-06T00:00:00Z')).toBe('2026-07-06')
    expect(dateInputValue('2026-07-06')).toBe('2026-07-06')
    expect(dateInputValue('')).toBe('')
    // Non-string (boolean / array) never crashes the date input.
    expect(dateInputValue(true)).toBe('')
    expect(dateInputValue(['a'])).toBe('')
  })

  it('emits an RFC3339 string from a native date input (string wire shape)', () => {
    const emitted = toRfc3339FromDateInput('2026-07-06')
    expect(emitted).toBe('2026-07-06T00:00:00Z')
    expect(typeof emitted).toBe('string')
    expect(toRfc3339FromDateInput('')).toBe('')
  })

  it('resolves "N units ago" to an absolute RFC3339 string', () => {
    const now = new Date('2026-07-06T12:34:56.000Z')
    expect(relativeDateValue(7, 'days', now)).toBe('2026-06-29T12:34:56Z')
    expect(relativeDateValue(2, 'weeks', now)).toBe('2026-06-22T12:34:56Z')
    expect(relativeDateValue(1, 'months', now)).toBe('2026-06-06T12:34:56Z')
    // No fractional-millis suffix, matching the stored received_at format.
    expect(relativeDateValue(1, 'days', now)).not.toContain('.')
  })
})

describe('ref + list value helpers (wire-shape parity)', () => {
  it('a picked mailbox/account/role ref is always a plain string', () => {
    expect(pickedRefValue('mbx-123')).toBe('mbx-123')
    expect(typeof pickedRefValue('mbx-123')).toBe('string')
    // The unset sentinel clears to the empty string (same as an empty text box).
    expect(pickedRefValue('__unset__')).toBe('')
  })

  it('the in-operator box still splits to a string[] exactly as before', () => {
    expect(splitListValue('a, b,,  c ')).toEqual(['a', 'b', 'c'])
    expect(splitListValue('')).toEqual([])
  })
})

describe('size value helpers (bytes wire shape)', () => {
  it('converts amount+unit to a byte-count string the numeric compiler parses', () => {
    expect(bytesFromSize(500, 'bytes')).toBe('500')
    expect(bytesFromSize(1, 'kb')).toBe('1024')
    expect(bytesFromSize(1, 'mb')).toBe(String(1024 * 1024))
    expect(bytesFromSize(2, 'mb')).toBe(String(2 * 1024 * 1024))
    // Always a string (parity with every other single-value operator).
    expect(typeof bytesFromSize(1, 'mb')).toBe('string')
  })

  it('emits the empty string for blank/invalid amounts, never NaN', () => {
    expect(bytesFromSize(Number.NaN, 'kb')).toBe('')
    expect(bytesFromSize(-1, 'kb')).toBe('')
  })

  it('round-trips a stored byte count to a friendly amount+unit', () => {
    expect(sizeInputParts(String(1024 * 1024))).toEqual({
      amount: '1',
      unit: 'mb',
    })
    expect(sizeInputParts('2048')).toEqual({ amount: '2', unit: 'kb' })
    expect(sizeInputParts('500')).toEqual({ amount: '500', unit: 'bytes' })
    expect(sizeInputParts('')).toEqual({ amount: '', unit: 'kb' })
    // Non-string (boolean/array) never crashes the size input.
    expect(sizeInputParts(true)).toEqual({ amount: '', unit: 'kb' })
  })
})

describe('emitted condition JSON — wire-shape parity vs the old text box', () => {
  // The widgets emit `{ ...condition, value }` where `value` is the transform's
  // output. This asserts each widget produces the SAME serialized shape the
  // generic text box produced, so the compiler/evaluator + stored JSON are
  // unchanged. This is the load-bearing assertion for R1.
  const base = (
    field: SmartMailboxField,
    op: SmartMailboxCondition['operator'],
    value: SmartMailboxCondition['value'],
  ): SmartMailboxCondition => ({
    type: 'condition',
    field,
    operator: op,
    negated: false,
    value,
  })

  it('date widget emits a string (was: hand-typed string)', () => {
    const emitted = {
      ...base('receivedAt', 'before', ''),
      value: toRfc3339FromDateInput('2026-07-06'),
    }
    expect(emitted).toEqual(
      base('receivedAt', 'before', '2026-07-06T00:00:00Z'),
    )
    expect(typeof emitted.value).toBe('string')
  })

  it('mailbox / account / role pickers emit a string (was: hand-typed id)', () => {
    expect({
      ...base('mailboxId', 'equals', ''),
      value: pickedRefValue('mbx-1'),
    }).toEqual(base('mailboxId', 'equals', 'mbx-1'))
    expect({
      ...base('sourceId', 'equals', ''),
      value: pickedRefValue('acct-1'),
    }).toEqual(base('sourceId', 'equals', 'acct-1'))
    expect({
      ...base('mailboxRole', 'equals', ''),
      value: pickedRefValue('inbox'),
    }).toEqual(base('mailboxRole', 'equals', 'inbox'))
  })

  it('the in operator still emits a string[] (unchanged)', () => {
    expect({
      ...base('subject', 'in', []),
      value: splitListValue('x, y'),
    }).toEqual(base('subject', 'in', ['x', 'y']))
  })
})
