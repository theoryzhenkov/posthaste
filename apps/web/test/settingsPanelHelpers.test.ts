import { describe, expect, it } from 'bun:test'

import type { AccountOverview } from '../src/api/types'
import { syncProgressLabel } from '../src/components/settings-panel/helpers'

function accountOverview(
  overrides: Partial<AccountOverview> = {},
): AccountOverview {
  return {
    id: 'primary',
    name: 'Primary',
    fullName: null,
    emailPatterns: ['primary@example.com'],
    driver: 'imapSmtp',
    enabled: true,
    appearance: {
      kind: 'initials',
      initials: 'P',
      colorHue: 210,
    },
    connection: {
      kind: 'manualCredentials',
      provider: 'generic',
      providerKind: 'generic',
      auth: 'password',
      baseUrl: null,
      username: 'primary@example.com',
      imap: null,
      smtp: null,
      secret: {
        storage: 'os',
        configured: true,
        label: null,
      },
    },
    createdAt: '2026-05-24T00:00:00Z',
    updatedAt: '2026-05-24T00:00:00Z',
    isDefault: false,
    status: 'syncing',
    push: 'connected',
    lastSyncAt: null,
    lastSyncError: null,
    lastSyncErrorCode: null,
    syncProgress: {
      syncId: 'sync-1',
      trigger: 'poll',
      startedAt: '2026-05-24T00:01:00Z',
      stage: 'fetching',
      detail: 'Syncing messages',
      mailboxName: 'Inbox',
      mailboxIndex: 1,
      mailboxCount: 2,
      messageCount: 10,
      totalCount: null,
    },
    ...overrides,
  }
}

describe('settings panel helper contracts', () => {
  // spec: docs/L1-api#account-crud-lifecycle
  it('only displays sync progress while the account status is syncing', () => {
    expect(syncProgressLabel(accountOverview())).toBe(
      'Syncing messages · Inbox · 1/2 · 10 messages',
    )

    expect(syncProgressLabel(accountOverview({ status: 'ready' }))).toBeNull()
  })
})
