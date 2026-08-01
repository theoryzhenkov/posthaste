/**
 * The stored detail-row arrangement, and the upgrade of the shape that came
 * before it.
 *
 * The store itself is module-scoped and reads storage once at import, so the
 * hooks are unreachable under `bun test`; `readDetailFields` is the part with
 * the decisions in it, and the one whose failure a reader would actually
 * notice — a bad migration silently rearranges the header of every message
 * they own.
 */
import { describe, expect, test } from 'bun:test'

import { detailFieldDefault } from '../fields'
import { moveDetailRow, readDetailFields } from './useFieldConfig'

describe('the old shape', () => {
  test('a flat list of ids becomes entries with declared presentation', () => {
    const upgraded = readDetailFields(['to', 'cc'])
    expect(upgraded).not.toBeNull()
    expect(upgraded?.map((field) => field.id)).toContain('to')
    expect(upgraded?.find((field) => field.id === 'to')).toEqual(
      detailFieldDefault('to'),
    )
  })

  test('keeps the reader’s own choices and their order', () => {
    const upgraded = readDetailFields(['replyTo', 'to'])
    const ids = upgraded?.map((field) => field.id) ?? []
    expect(ids.indexOf('replyTo')).toBeLessThan(ids.indexOf('to'))
  })

  test('adds the rows the old shape could not name', () => {
    // Before the rework the header drew the subject, sender and tags itself,
    // so no stored selection mentions them; honouring one literally would
    // produce a message that says neither what it is nor who sent it.
    const ids = readDetailFields(['to'])?.map((field) => field.id) ?? []
    expect(ids).toContain('subject')
    expect(ids).toContain('from')
    expect(ids).toContain('tags')
    expect(ids).toContain('to')
  })

  test('drops an id the detail surface never shows', () => {
    const ids = readDetailFields(['preview', 'to'])?.map((f) => f.id) ?? []
    expect(ids).not.toContain('preview')
    expect(ids).toContain('to')
  })
})

describe('the current shape', () => {
  test('is taken as written, order included', () => {
    const stored = [
      { id: 'to', emphasis: 'meta', showLabel: false },
      { id: 'subject', emphasis: 'heading', showLabel: false },
    ]
    expect(readDetailFields(stored)).toEqual([
      { id: 'to', emphasis: 'meta', showLabel: false },
      { id: 'subject', emphasis: 'heading', showLabel: false },
    ])
  })

  test('an entry list is NOT topped up with structural rows', () => {
    // Unlike the old shape, this one can say "no subject row" and mean it.
    const ids =
      readDetailFields([{ id: 'to', emphasis: 'body', showLabel: true }])?.map(
        (field) => field.id,
      ) ?? []
    expect(ids).toEqual(['to'])
  })

  test('an entry naming an unknown field is dropped', () => {
    expect(readDetailFields([{ id: 'nope' }])).toEqual([])
  })

  test('an empty list is an empty header, not a reset', () => {
    // It carries no evidence of either shape, so it reads as the new one:
    // turning every row off in settings must survive a relaunch.
    expect(readDetailFields([])).toEqual([])
  })

  test('a bad part costs that part only', () => {
    const stored = [{ id: 'subject', emphasis: 'enormous', showLabel: 'yes' }]
    expect(readDetailFields(stored)).toEqual([detailFieldDefault('subject')])
  })

  test('a repeated id is stored once', () => {
    const stored = [
      { id: 'to', emphasis: 'meta', showLabel: false },
      { id: 'to', emphasis: 'body', showLabel: true },
    ]
    expect(readDetailFields(stored)).toEqual([
      { id: 'to', emphasis: 'meta', showLabel: false },
    ])
  })
})

describe('junk', () => {
  test('anything that is not a list has no arrangement in it', () => {
    // `null` tells the caller to fall back to the defaults, which is not the
    // same as an empty arrangement.
    expect(readDetailFields(undefined)).toBeNull()
    expect(readDetailFields('to,cc')).toBeNull()
    expect(readDetailFields(7)).toBeNull()
    expect(readDetailFields({ id: 'to' })).toBeNull()
  })
})

describe('reordering', () => {
  const rows = (['subject', 'from', 'to'] as const).map(detailFieldDefault)
  const ids = (fields: typeof rows) => fields.map((field) => field.id)

  test('a row swaps with its neighbour', () => {
    expect(ids(moveDetailRow(rows, 'from', -1))).toEqual([
      'from',
      'subject',
      'to',
    ])
    expect(ids(moveDetailRow(rows, 'from', 1))).toEqual([
      'subject',
      'to',
      'from',
    ])
  })

  test('the ends do not overshoot', () => {
    expect(moveDetailRow(rows, 'subject', -1)).toBe(rows)
    expect(moveDetailRow(rows, 'to', 1)).toBe(rows)
  })

  test('a row that is not shown moves nothing', () => {
    expect(moveDetailRow(rows, 'bcc', -1)).toBe(rows)
  })
})
