import { describe, expect, it } from 'bun:test'

import {
  normalizeValidAppliedSearchQuery,
  prepareServerSearchQuery,
} from '../src/searchQuery'

describe('server search query preparation', () => {
  it('normalizes valid queries before server use', () => {
    expect(prepareServerSearchQuery('  from:   Posthaste  ')).toEqual({
      query: 'from: Posthaste',
      validation: { state: 'valid' },
      isBlocked: false,
    })
  })

  it('does not send incomplete or invalid filters to the server', () => {
    expect(prepareServerSearchQuery('tag:')).toMatchObject({
      query: undefined,
      validation: { state: 'incomplete' },
      isBlocked: true,
    })
    expect(prepareServerSearchQuery('is: readish')).toMatchObject({
      query: undefined,
      validation: { state: 'invalid' },
      isBlocked: true,
    })
  })

  it('only normalizes valid queries for applied filter state', () => {
    expect(normalizeValidAppliedSearchQuery('  from:   Posthaste  ')).toBe(
      'from: Posthaste',
    )
    expect(normalizeValidAppliedSearchQuery('   ')).toBe('')
    expect(normalizeValidAppliedSearchQuery('tag:')).toBeNull()
  })
})
