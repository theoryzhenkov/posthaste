import { describe, expect, it } from 'bun:test'

import {
  accountHealth,
  accountHealthFor,
  unhealthyAccounts,
} from '../src/accountHealth'
import type { AccountOverview, AccountRuntime } from '../src/api/types'

function runtime(overrides: Partial<AccountRuntime> = {}): AccountRuntime {
  return {
    status: 'ready',
    push: 'connected',
    lastSyncAt: null,
    lastSyncError: null,
    lastSyncErrorCode: null,
    syncProgress: null,
    ...overrides,
  }
}

function account(overrides: Partial<AccountOverview> = {}): AccountOverview {
  return {
    id: 'primary',
    name: 'Primary',
    fullName: null,
    signature: null,
    emailPatterns: ['primary@example.com'],
    driver: 'imapSmtp',
    enabled: true,
    appearance: { kind: 'initials', initials: 'P', colorHue: 200 },
    connection: {
      kind: 'managedOAuth',
      provider: 'gmail',
      providerKind: 'gmail',
      auth: 'oauth2',
      username: 'primary@gmail.com',
      imap: null,
      smtp: null,
      secret: { storage: 'os', configured: true, label: null },
    },
    createdAt: '2026-04-28T12:00:00Z',
    updatedAt: '2026-04-28T12:00:00Z',
    isDefault: true,
    runtime: runtime(),
    ...overrides,
  }
}

describe('account health presentation (M45)', () => {
  it('reports healthy states with no action and no message', () => {
    expect(accountHealth(runtime({ status: 'ready' }), 'Gmail')).toMatchObject({
      isUnhealthy: false,
      message: null,
      action: null,
      label: 'Connected',
    })
    expect(
      accountHealth(runtime({ status: 'syncing' }), 'Gmail'),
    ).toMatchObject({ isUnhealthy: false, autoRetrying: true, action: null })
  })

  it('classifies a TCP/network offline error as a retrying network issue', () => {
    const health = accountHealth(
      runtime({
        status: 'offline',
        lastSyncErrorCode: 'network_error',
        // Even if a raw string were present, the presentation never uses it.
        lastSyncError: 'network error: cannot connect to TCP stream',
      }),
      'Gmail',
    )
    expect(health.category).toBe('network')
    expect(health.isUnhealthy).toBe(true)
    expect(health.autoRetrying).toBe(true)
    expect(health.action).toBe('retry')
    expect(health.message).toContain('Gmail')
    expect(health.message).not.toContain('TCP stream')
  })

  it('classifies an auth error as a reconnect action', () => {
    const health = accountHealth(
      runtime({ status: 'authError', lastSyncErrorCode: 'auth_error' }),
      'Gmail',
    )
    expect(health.category).toBe('auth')
    expect(health.action).toBe('reconnect')
    expect(health.actionLabel).toBe('Reconnect')
    expect(health.autoRetrying).toBe(false)
    expect(health.message?.toLowerCase()).toContain('reconnect')
  })

  it('classifies a rate-limit code as a throttled, auto-retrying state', () => {
    const health = accountHealth(
      runtime({ status: 'degraded', lastSyncErrorCode: 'rate_limited' }),
      'Gmail',
    )
    expect(health.category).toBe('rateLimited')
    expect(health.autoRetrying).toBe(true)
    expect(health.message).toContain('throttling')
  })

  it('classifies a gateway rejection as a config issue to edit', () => {
    const health = accountHealth(
      runtime({ status: 'degraded', lastSyncErrorCode: 'gateway_rejected' }),
      'Gmail',
    )
    expect(health.category).toBe('config')
    expect(health.action).toBe('edit')
  })

  it('falls back to an internal degraded state for unknown codes', () => {
    const health = accountHealth(
      runtime({ status: 'degraded', lastSyncErrorCode: 'arm_timeout' }),
      'Gmail',
    )
    expect(health.category).toBe('internal')
    expect(health.isUnhealthy).toBe(true)
    expect(health.message).toBeTruthy()
  })

  it('uses the provider display name for phrasing', () => {
    const health = accountHealthFor(
      account({
        runtime: runtime({
          status: 'offline',
          lastSyncErrorCode: 'network_error',
        }),
      }),
    )
    expect(health.message).toContain('Gmail')
  })

  it('selects only enabled, unhealthy accounts for the global indicator', () => {
    const healthy = account({ id: 'a', runtime: runtime({ status: 'ready' }) })
    const broken = account({
      id: 'b',
      runtime: runtime({
        status: 'offline',
        lastSyncErrorCode: 'network_error',
      }),
    })
    const disabledBroken = account({
      id: 'c',
      enabled: false,
      runtime: runtime({
        status: 'offline',
        lastSyncErrorCode: 'network_error',
      }),
    })
    expect(
      unhealthyAccounts([healthy, broken, disabledBroken]).map((a) => a.id),
    ).toEqual(['b'])
  })
})
