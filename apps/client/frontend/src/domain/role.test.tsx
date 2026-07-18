import { describe, expect, test } from 'bun:test'

import { KNOWN_MAILBOX_ROLES } from './vocabulary'
import { parseMailboxRole } from './role'

describe('parseMailboxRole', () => {
  test('accepts every known role', () => {
    for (const role of KNOWN_MAILBOX_ROLES) {
      expect(parseMailboxRole(role)).toBe(role)
    }
  })

  test('is idempotent on its own output', () => {
    const parsed = parseMailboxRole('inbox')
    expect(parsed).not.toBeNull()
    expect(parseMailboxRole(parsed)).toBe(parsed)
  })

  test('rejects unknown, empty, and absent roles', () => {
    expect(parseMailboxRole('important')).toBeNull()
    expect(parseMailboxRole('Inbox')).toBeNull() // roles are lowercase on the wire
    expect(parseMailboxRole('')).toBeNull()
    expect(parseMailboxRole(null)).toBeNull()
    expect(parseMailboxRole(undefined)).toBeNull()
  })
})
