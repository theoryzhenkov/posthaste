import { describe, expect, test } from 'bun:test'

import {
  formatDiagnosticsBundle,
  maskEmails,
  redactSecrets,
  summarizeAccount,
} from './diagnostics'
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

describe('sanitization', () => {
  test('masks email addresses', () => {
    expect(maskEmails('mail to a.b@example.com failed')).toBe(
      'mail to [email] failed',
    )
  })

  test('redacts bearer tokens and key-value secrets', () => {
    expect(redactSecrets('Bearer abcdefgh12345678')).toBe('[redacted]')
    expect(redactSecrets('password=hunter42')).toBe('[redacted]')
  })
})

describe('summarizeAccount', () => {
  test('keeps structural fields only and sanitizes the error', () => {
    const summary = summarizeAccount(
      row({
        status: 'degraded',
        lastSyncError: 'IMAP login failed for a.b@example.com',
      }),
    )
    expect(summary.status).toBe('degraded')
    expect(summary.lastSyncError).toBe('IMAP login failed for [email]')
    expect(summary).not.toHaveProperty('name')
  })
})

describe('formatDiagnosticsBundle', () => {
  test('renders account lines without identity', () => {
    const bundle = formatDiagnosticsBundle({
      appVersion: '1.0.0',
      releaseChannel: 'stable',
      os: 'macOS',
      arch: 'arm64',
      logDirPath: null,
      accounts: [row({ status: 'offline', enabled: false })],
      generatedAt: new Date('2026-01-01T00:00:00Z'),
    })
    expect(bundle).toContain('Accounts (1):')
    expect(bundle).toContain('disabled offline (push: connected)')
    expect(bundle).not.toContain('Work')
  })
})
