import { describe, expect, test } from 'bun:test'

import {
  accountHealth,
  accountHealthFor,
  classifyAccountSetupError,
  unhealthyAccounts,
} from './accountHealth'
import { MailApiError } from '../transport/client'
import type { AccountRow } from '@/gen'

function row(overrides: Partial<AccountRow> = {}): AccountRow {
  return {
    id: 'acc-1',
    name: 'Work',
    fullName: null,
    enabled: true,
    isDefault: false,
    status: 'ready',
    push: 'connected',
    lastSyncAt: null,
    lastSyncError: null,
    ...overrides,
  }
}

describe('accountHealth', () => {
  test('healthy statuses carry no message or action', () => {
    for (const status of ['ready', 'syncing', 'disabled'] as const) {
      const health = accountHealth(row({ status }), 'Work')
      expect(health.isUnhealthy).toBe(false)
      expect(health.message).toBeNull()
      expect(health.action).toBeNull()
    }
  })

  test('syncing displays as an informational Syncing state', () => {
    const health = accountHealth(row({ status: 'syncing' }), 'Work')
    expect(health.label).toBe('Syncing')
    expect(health.severity).toBe('info')
    expect(health.autoRetrying).toBe(true)
    expect(health.isUnhealthy).toBe(false)
  })

  test('the display state derives from the row alone: a refetched ready row shows Connected with no syncing remnant', () => {
    // The stuck-"Syncing" regression contract: the read model holds no
    // state, so re-deriving from a row whose wire status returned to rest
    // must fully clear the syncing presentation.
    expect(accountHealth(row({ status: 'syncing' }), 'Work').label).toBe(
      'Syncing',
    )
    const settled = accountHealth(
      row({ status: 'ready', lastSyncAt: '2026-07-19T00:00:00Z' }),
      'Work',
    )
    expect(settled.label).toBe('Connected')
    expect(settled.severity).toBe('ok')
    expect(settled.autoRetrying).toBe(false)
    expect(settled.isUnhealthy).toBe(false)
  })

  test('authError presents as sign-in needed with reconnect', () => {
    const health = accountHealth(row({ status: 'authError' }), 'Work')
    expect(health.category).toBe('auth')
    expect(health.severity).toBe('error')
    expect(health.action).toBe('reconnect')
    expect(health.isUnhealthy).toBe(true)
  })

  test('offline presents as a network issue with provider phrasing', () => {
    const health = accountHealth(row({ status: 'offline' }), 'Gmail')
    expect(health.category).toBe('network')
    expect(health.message).toContain('Gmail')
    expect(health.autoRetrying).toBe(true)
  })

  test('degraded prefers the server message when present', () => {
    const health = accountHealth(
      row({ status: 'degraded', lastSyncError: 'Mailbox quota exceeded' }),
      'Work',
    )
    expect(health.category).toBe('internal')
    expect(health.message).toBe('Mailbox quota exceeded')
  })

  test('degraded without a server message falls back to generic copy', () => {
    const health = accountHealth(row({ status: 'degraded' }), 'Work')
    expect(health.message).toContain('Something went wrong')
  })

  test('unhealthyAccounts skips disabled accounts', () => {
    const rows = [
      row({ id: 'a', status: 'authError' }),
      row({ id: 'b', status: 'authError', enabled: false }),
      row({ id: 'c', status: 'ready' }),
    ]
    expect(unhealthyAccounts(rows).map((r) => r.id)).toEqual(['a'])
    expect(accountHealthFor(rows[0]!).isUnhealthy).toBe(true)
  })
})

describe('classifyAccountSetupError', () => {
  const err = (kind: MailApiError['kind'], message = 'nope') =>
    new MailApiError({ kind, message, retryable: false }, 400)

  test('unauthorized classifies as auth and appends the app-password hint', () => {
    const result = classifyAccountSetupError(
      err('unauthorized'),
      'Use an app password.',
    )
    expect(result.category).toBe('auth')
    expect(result.message).toContain('Use an app password.')
  })

  test('malformedRequest surfaces the server validation message', () => {
    const result = classifyAccountSetupError(
      err('malformedRequest', 'A username is required.'),
    )
    expect(result.category).toBe('config')
    expect(result.message).toBe('A username is required.')
  })

  test('unavailable classifies as network', () => {
    expect(classifyAccountSetupError(err('unavailable')).category).toBe(
      'network',
    )
  })

  test('non-API errors classify as internal', () => {
    expect(classifyAccountSetupError(new Error('boom')).category).toBe(
      'internal',
    )
  })
})
