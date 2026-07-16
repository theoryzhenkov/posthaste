import { describe, it, expect } from 'bun:test'

import {
  formatDiagnosticsBundle,
  maskEmails,
  redactSecrets,
  sanitizeText,
  summarizeAccount,
} from '../src/diagnostics'
import type {
  AccountOverview,
  DiagnosticsBundleInput,
} from '../src/diagnostics'

/** Minimal valid `AccountOverview` for the pure-function tests (no react-query). */
function accountOverview(
  overrides: Partial<AccountOverview> = {},
): AccountOverview {
  return {
    id: 'primary',
    name: 'Primary',
    fullName: null,
    emailPatterns: ['*@example.com'],
    driver: 'mock',
    enabled: true,
    appearance: { kind: 'initials', initials: 'P', colorHue: 120 },
    connection: {
      kind: 'manualCredentials',
      provider: 'generic',
      providerKind: 'generic',
      auth: 'password',
      username: 'primary@example.com',
      imap: null,
      smtp: null,
      secret: { storage: 'env', configured: false, label: null },
      baseUrl: null,
    },
    createdAt: '2026-06-29T00:00:00Z',
    updatedAt: '2026-06-29T00:00:00Z',
    isDefault: true,
    runtime: {
      status: 'ready',
      push: 'connected',
      lastSyncAt: '2026-06-29T00:00:00Z',
      lastSyncError: null,
      lastSyncErrorCode: null,
      syncProgress: null,
    },
    ...overrides,
  }
}

describe('maskEmails', () => {
  it('replaces email addresses with [email]', () => {
    expect(maskEmails('contact user@example.com')).toBe('contact [email]')
    expect(maskEmails('a@b.co and c@d.io')).toBe('[email] and [email]')
  })

  it('leaves non-email text unchanged', () => {
    expect(maskEmails('no addresses here')).toBe('no addresses here')
  })
})

describe('redactSecrets', () => {
  it('redacts labeled bearer tokens + passwords', () => {
    expect(redactSecrets('Authorization: Bearer abcdef1234567890')).toBe(
      'Authorization: [redacted]',
    )
    expect(redactSecrets('password=hunter2pass1234')).toBe('[redacted]')
    expect(redactSecrets('api_key=sk_live_0123456789abcdef')).toBe('[redacted]')
  })

  it('redacts long opaque base64-ish blobs', () => {
    const token = 'A'.repeat(48)
    expect(redactSecrets(`token ${token}`)).toBe('token [redacted]')
  })

  it('leaves ordinary short text intact', () => {
    expect(redactSecrets('connection reset by peer')).toBe(
      'connection reset by peer',
    )
  })
})

describe('sanitizeText', () => {
  it('masks emails + redacts secrets together', () => {
    expect(
      sanitizeText('auth fail for user@example.com token=ABCDEFGHIJ1234567890'),
    ).toBe('auth fail for [email] [redacted]')
  })
})

describe('summarizeAccount', () => {
  it('projects structural fields + sanitizes the error message', () => {
    const summary = summarizeAccount(
      accountOverview({
        driver: 'imapSmtp',
        enabled: false,
        runtime: {
          status: 'authError',
          push: 'unsupported',
          lastSyncAt: null,
          lastSyncError:
            'invalid creds for admin@example.com (token=SECRET1234567890)',
          lastSyncErrorCode: 'E_AUTH',
          syncProgress: null,
        },
      }),
    )
    expect(summary).toEqual({
      driver: 'imapSmtp',
      enabled: false,
      status: 'authError',
      push: 'unsupported',
      lastSyncErrorCode: 'E_AUTH',
      lastSyncError: 'invalid creds for [email] ([redacted])',
    })
  })

  it('nulls out a missing error message', () => {
    expect(summarizeAccount(accountOverview()).lastSyncError).toBeNull()
  })
})

describe('formatDiagnosticsBundle', () => {
  function bundle(overrides: Partial<DiagnosticsBundleInput> = {}): string {
    return formatDiagnosticsBundle({
      appVersion: '0.2.0',
      releaseChannel: 'nightly',
      os: 'linux',
      arch: 'x86_64',
      logDirPath: '/home/user/.local/share/posthaste/logs',
      accounts: [accountOverview()],
      generatedAt: new Date('2026-06-29T00:00:00.000Z'),
      ...overrides,
    })
  }

  it('includes version, platform, account status + log location', () => {
    const text = bundle()
    expect(text).toContain('Version: 0.2.0 (nightly)')
    expect(text).toContain('Platform: linux x86_64')
    expect(text).toContain('Accounts (1):')
    expect(text).toContain('[mock] ready')
    expect(text).toContain(
      'Log location: /home/user/.local/share/posthaste/logs',
    )
  })

  it('handles zero accounts', () => {
    const text = bundle({ accounts: [] })
    expect(text).toContain('Accounts (0):')
    expect(text).toContain('(none configured)')
  })

  it('omits the arch when absent', () => {
    expect(bundle({ arch: '' })).toContain('Platform: linux')
  })

  it('never leaks emails or secrets from an account error message', () => {
    const text = bundle({
      accounts: [
        accountOverview({
          runtime: {
            status: 'degraded',
            push: 'reconnecting',
            lastSyncAt: null,
            lastSyncError:
              'IMAP failure for user@example.com Authorization: Bearer ZmFrZXRva2VuMTIzNDU2Nzg5MDEyMzQ1',
            lastSyncErrorCode: 'E_IMAP',
            syncProgress: null,
          },
        }),
      ],
    })
    expect(text).not.toContain('user@example.com')
    expect(text).not.toContain('ZmFrZXRva2VuMTIzNDU2Nzg5MDEyMzQ1')
    expect(text).toContain('[email]')
    expect(text).toContain('[redacted]')
    expect(text).toContain('E_IMAP')
  })
})
