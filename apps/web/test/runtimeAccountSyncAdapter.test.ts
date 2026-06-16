import { afterEach, describe, expect, it, spyOn } from 'bun:test'

import * as apiClient from '../src/api/client'
import type { AccountOverview } from '../src/api/types'
import {
  fetchRuntimeAccounts,
  resetRuntimeAdapterForTesting,
  setRuntimeAdapterForTesting,
  triggerRuntimeSync,
} from '../src/runtime/adapter'
import { createFakeRuntimeAdapter } from '../src/runtime/fakeAdapter'

const account: AccountOverview = {
  id: 'primary',
  name: 'Primary',
  fullName: null,
  emailPatterns: ['primary@example.com'],
  driver: 'mock',
  enabled: true,
  appearance: { kind: 'initials', initials: 'P', colorHue: 200 },
  connection: {
    kind: 'manualCredentials',
    provider: 'generic',
    providerKind: 'generic',
    auth: 'password',
    baseUrl: null,
    username: 'primary@example.com',
    imap: null,
    smtp: null,
    secret: { storage: 'os', configured: true, label: null },
  },
  createdAt: '2026-04-28T12:00:00Z',
  updatedAt: '2026-04-28T12:00:00Z',
  isDefault: true,
  status: 'ready',
  push: 'disabled',
  lastSyncAt: null,
  lastSyncError: null,
  lastSyncErrorCode: null,
  syncProgress: null,
}

afterEach(() => {
  resetRuntimeAdapterForTesting()
})

describe('runtime account and sync adapter', () => {
  it('dispatches account reads and sync through a fake adapter without a backend', async () => {
    const fake = createFakeRuntimeAdapter()
    const syncResult = { ok: true, eventCount: 3, mode: 'incremental' as const }
    fake.queueAccounts([account])
    fake.queueSyncResult(syncResult)
    setRuntimeAdapterForTesting(fake)

    expect(await fetchRuntimeAccounts()).toEqual([account])
    expect(await triggerRuntimeSync({ sourceId: 'primary' })).toBe(syncResult)
    expect(fake.accountCalls).toBe(1)
    expect(fake.syncCalls).toEqual([{ sourceId: 'primary' }])
  })

  it('wraps existing HTTP account reads and sync by default', async () => {
    const accountsSpy = spyOn(apiClient, 'fetchAccounts').mockResolvedValue([
      account,
    ])
    const syncResult = {
      ok: true,
      eventCount: 1,
      mode: 'fullMetadata' as const,
    }
    const syncSpy = spyOn(apiClient, 'triggerSync').mockResolvedValue(
      syncResult,
    )

    try {
      expect(await fetchRuntimeAccounts()).toEqual([account])
      expect(
        await triggerRuntimeSync({
          sourceId: 'primary',
          mode: 'fullMetadata',
        }),
      ).toBe(syncResult)
      expect(accountsSpy).toHaveBeenCalledWith()
      expect(syncSpy).toHaveBeenCalledWith({
        sourceId: 'primary',
        mode: 'fullMetadata',
      })
    } finally {
      accountsSpy.mockRestore()
      syncSpy.mockRestore()
    }
  })
})
