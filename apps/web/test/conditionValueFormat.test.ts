import { describe, expect, it } from 'bun:test'

import type { SmartMailboxCondition, SmartMailboxField } from '../src/api/types'
import {
  ALL_QUERY_FIELDS,
  QUERY_FIELD_SCHEMA,
} from '../src/api/querySchema.gen'
import {
  FIELD_REGISTRY,
  operatorLabel,
  operatorLabelForField,
  operatorOptionsForField,
  valueTypeForField,
} from '../src/components/settings-panel/helpers/fieldRegistry'
import {
  absoluteDateValue,
  bytesFromSize,
  dateInputValue,
  dateValueMode,
  pickedRefValue,
  relativeDateValue,
  relativeParts,
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
      conversationId: ['equals', 'in'],
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
      receivedAt: ['lt', 'gt', 'le', 'ge'],
      size: ['lt', 'gt', 'le', 'ge'],
    }
    for (const field of Object.keys(expected) as SmartMailboxField[]) {
      expect(operatorOptionsForField(field)).toEqual(expected[field])
    }
  })

  it('covers exactly the fields in the generated Rust schema', () => {
    // The field set is generated from the canonical Rust schema, so the registry
    // must cover exactly `ALL_QUERY_FIELDS` — no hand-maintained subset that can
    // drift (this is what caught `conversationId` missing from the web registry).
    expect(Object.keys(FIELD_REGISTRY).sort()).toEqual(
      [...ALL_QUERY_FIELDS].sort(),
    )
  })

  it('D6: labels the neutral operators per field type (date vs size)', () => {
    // The MODEL operators are neutral (`lt`/`gt`/`le`/`ge`); the editor labels
    // them per value type. A date field reads "before/after/on or before/on or
    // after"; a size field reads "smaller/larger than / at most / at least".
    // Both map to the SAME neutral operators.
    expect(operatorLabelForField('receivedAt', 'lt')).toBe('before')
    expect(operatorLabelForField('receivedAt', 'gt')).toBe('after')
    expect(operatorLabelForField('receivedAt', 'le')).toBe('on or before')
    expect(operatorLabelForField('receivedAt', 'ge')).toBe('on or after')

    expect(operatorLabelForField('size', 'lt')).toBe('smaller than')
    expect(operatorLabelForField('size', 'gt')).toBe('larger than')
    expect(operatorLabelForField('size', 'le')).toBe('at most')
    expect(operatorLabelForField('size', 'ge')).toBe('at least')

    // Type-agnostic operators keep their plain labels regardless of value type.
    expect(operatorLabel('equals', 'text')).toBe('equals')
    expect(operatorLabel('contains', 'text')).toBe('contains')
    expect(operatorLabel('in', 'text')).toBe('is one of')
  })

  it('derives its operators verbatim from the generated Rust schema', () => {
    // The schema-consistency guard: the operators the editor offers for every
    // field are exactly the generated schema's, so they cannot drift from the
    // store SQL compiler (which validates against the SAME schema).
    for (const field of ALL_QUERY_FIELDS) {
      expect(operatorOptionsForField(field)).toEqual([
        ...QUERY_FIELD_SCHEMA[field].operators,
      ])
    }
  })
})

describe('date value helpers', () => {
  it('extracts YYYY-MM-DD from a legacy bare string AND a typed absolute date', () => {
    // Legacy bare RFC3339 string (pre-R5a stored value) still loads for editing.
    expect(dateInputValue('2026-07-06T00:00:00Z')).toBe('2026-07-06')
    expect(dateInputValue('2026-07-06')).toBe('2026-07-06')
    // Typed absolute date object.
    expect(
      dateInputValue({ kind: 'absolute', value: '2026-07-06T00:00:00Z' }),
    ).toBe('2026-07-06')
    expect(dateInputValue('')).toBe('')
    // Non-string (boolean / array / relative) never crashes the date input.
    expect(dateInputValue(true)).toBe('')
    expect(dateInputValue(['a'])).toBe('')
    expect(dateInputValue({ kind: 'relative', amount: 7, unit: 'days' })).toBe(
      '',
    )
  })

  it('emits an RFC3339 string from a native date input', () => {
    const emitted = toRfc3339FromDateInput('2026-07-06')
    expect(emitted).toBe('2026-07-06T00:00:00Z')
    expect(typeof emitted).toBe('string')
    expect(toRfc3339FromDateInput('')).toBe('')
  })

  it('the absolute widget emits a typed { kind:"absolute" } value', () => {
    expect(absoluteDateValue('2026-07-06')).toEqual({
      kind: 'absolute',
      value: '2026-07-06T00:00:00Z',
    })
  })

  it('the relative widget stores { kind:"relative", amount, unit } AS-IS — NOT a frozen date', () => {
    // The bug fix: no `new Date()` freeze. The offset is persisted verbatim so
    // the evaluator rolls it against `now` at query time.
    const emitted = relativeDateValue(7, 'days')
    expect(emitted).toEqual({ kind: 'relative', amount: 7, unit: 'days' })
    // It must NOT be a resolved absolute string (the old freeze behavior).
    expect(typeof emitted).not.toBe('string')
    // Normalizes a blank/negative amount to 0, never NaN.
    expect(relativeDateValue(Number.NaN, 'weeks')).toEqual({
      kind: 'relative',
      amount: 0,
      unit: 'weeks',
    })
  })

  it('classifies stored values into the right sub-editor mode', () => {
    expect(dateValueMode({ kind: 'relative', amount: 7, unit: 'days' })).toBe(
      'relative',
    )
    expect(
      dateValueMode({ kind: 'absolute', value: '2026-07-06T00:00:00Z' }),
    ).toBe('absolute')
    // A legacy bare string edits as absolute.
    expect(dateValueMode('2026-07-06T00:00:00Z')).toBe('absolute')
  })

  it('reads a stored relative value back into editable amount/unit parts', () => {
    expect(
      relativeParts({ kind: 'relative', amount: 3, unit: 'weeks' }),
    ).toEqual({
      amount: '3',
      unit: 'weeks',
    })
    // A non-relative value defaults to 7 days.
    expect(relativeParts('2026-07-06T00:00:00Z')).toEqual({
      amount: '7',
      unit: 'days',
    })
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

  it('absolute date widget emits a typed { kind:"absolute" } value', () => {
    const emitted = {
      ...base('receivedAt', 'lt', ''),
      value: absoluteDateValue('2026-07-06'),
    }
    expect(emitted).toEqual(
      base('receivedAt', 'lt', {
        kind: 'absolute',
        value: '2026-07-06T00:00:00Z',
      }),
    )
  })

  it('relative date widget emits a rolling { kind:"relative" } value (no freeze)', () => {
    const emitted = {
      ...base('receivedAt', 'gt', ''),
      value: relativeDateValue(7, 'days'),
    }
    expect(emitted).toEqual(
      base('receivedAt', 'gt', {
        kind: 'relative',
        amount: 7,
        unit: 'days',
      }),
    )
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
