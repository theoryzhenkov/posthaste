import { describe, expect, test } from 'bun:test'

import { conversationViewQuery, parseSearchQuery } from './query'

describe('parseSearchQuery', () => {
  test('accepts plain text and known prefixes, normalizing whitespace', () => {
    expect<string | null>(parseSearchQuery('hello   world')).toBe('hello world')
    expect<string | null>(parseSearchQuery('  is:unread from:theo ')).toBe(
      'is:unread from:theo',
    )
    expect<string | null>(parseSearchQuery('date:2026-07-18')).toBe('date:2026-07-18')
  })

  test('empty input parses to the empty (clear) query', () => {
    expect<string | null>(parseSearchQuery('')).toBe('')
    expect<string | null>(parseSearchQuery('   ')).toBe('')
  })

  test('is idempotent on its own output', () => {
    const parsed = parseSearchQuery('is:unread   hello')
    expect(parsed).not.toBeNull()
    expect(parseSearchQuery(parsed as string)).toBe(parsed)
  })

  test('rejects queries the grammar blocks', () => {
    expect(parseSearchQuery('is:not-a-state')).toBeNull()
    expect(parseSearchQuery('date:2026-02-30')).toBeNull()
    expect(parseSearchQuery('newer:2x')).toBeNull()
  })

  test('conversationViewQuery output is a valid query by construction', () => {
    const query = conversationViewQuery('conv-123')
    expect<string>(query).toBe('conversation:conv-123')
    expect(parseSearchQuery(query)).toBe(query)
  })
})
