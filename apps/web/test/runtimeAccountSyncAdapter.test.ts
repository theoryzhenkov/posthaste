import { afterEach, describe, expect, it, spyOn } from 'bun:test'

import * as apiClient from '../src/api/client'
import type { AccountOverview, CreateAccountInput } from '../src/api/types'
import {
  resetRuntimeAdapterForTesting,
  setRuntimeAdapterForTesting,
} from '../src/runtime/adapter'
import { createFakeRuntimeAdapter } from '../src/runtime/fakeAdapter'
import { runtimeMutations } from '../src/runtime/mutations'
import { runtimeViews } from '../src/runtime/views'

const createInput: CreateAccountInput = {
  name: 'Primary',
  emailPatterns: ['primary@example.com'],
  transport: {
    provider: 'generic',
    auth: 'password',
    baseUrl: 'https://mail.example.test',
    username: 'primary@example.com',
  },
  secret: { mode: 'replace', password: 'secret' },
}

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

    expect(await runtimeViews.accounts.list()).toEqual([account])
    expect(await runtimeMutations.accounts.sync({ sourceId: 'primary' })).toBe(
      syncResult,
    )
    expect(fake.accountCalls).toBe(1)
    expect(fake.syncCalls).toEqual([{ sourceId: 'primary' }])
  })

  it('dispatches account mutations through a fake adapter without a backend', async () => {
    const fake = createFakeRuntimeAdapter({
      defaultAccount: account,
      defaultOAuthStartResponse: {
        authorizationUrl: 'https://accounts.example.test/auth',
        state: 'state-1',
        redirectUri: 'http://localhost:3001/v1/oauth/callback',
      },
      defaultVerificationResponse: {
        ok: true,
        identityEmail: 'primary@example.com',
        pushSupported: false,
      },
    })
    setRuntimeAdapterForTesting(fake)

    const logo = new File(['logo'], 'logo.png', { type: 'image/png' })
    await expect(runtimeViews.accounts.detail('primary')).resolves.toBe(account)
    await expect(runtimeMutations.accounts.create(createInput)).resolves.toBe(
      account,
    )
    await expect(
      runtimeMutations.accounts.update('primary', { name: 'Renamed' }),
    ).resolves.toBe(account)
    await expect(
      runtimeMutations.accounts.uploadLogo('primary', logo),
    ).resolves.toBe(account)
    await expect(runtimeMutations.accounts.verify('primary')).resolves.toEqual({
      ok: true,
      identityEmail: 'primary@example.com',
      pushSupported: false,
    })
    await expect(runtimeMutations.accounts.enable('primary')).resolves.toEqual({
      ok: true,
    })
    await expect(runtimeMutations.accounts.disable('primary')).resolves.toEqual(
      {
        ok: true,
      },
    )
    await expect(runtimeMutations.accounts.delete('primary')).resolves.toEqual({
      ok: true,
    })
    await expect(
      runtimeMutations.oauth.startProvider({
        provider: 'gmail',
        clientId: 'client-1',
        redirectUri: runtimeViews.oauth.redirectUri(),
      }),
    ).resolves.toEqual({
      authorizationUrl: 'https://accounts.example.test/auth',
      state: 'state-1',
      redirectUri: 'http://localhost:3001/v1/oauth/callback',
    })
    expect(fake.accountDetailCalls).toEqual(['primary'])
    expect(fake.accountCreateCalls).toEqual([createInput])
    expect(fake.accountUpdateCalls).toEqual([
      { accountId: 'primary', input: { name: 'Renamed' } },
    ])
    expect(fake.accountLogoUploadCalls).toEqual([
      { accountId: 'primary', file: logo },
    ])
    expect(fake.accountVerificationCalls).toEqual(['primary'])
    expect(fake.accountCommandCalls).toEqual([
      { kind: 'enable', accountId: 'primary' },
      { kind: 'disable', accountId: 'primary' },
      { kind: 'delete', accountId: 'primary' },
    ])
    expect(fake.oauthStartCalls).toEqual([
      {
        provider: 'gmail',
        clientId: 'client-1',
        redirectUri: runtimeViews.oauth.redirectUri(),
        hasClientSecret: false,
      },
    ])
  })

  it('wraps existing HTTP account mutations by default', async () => {
    const fetchSpy = spyOn(apiClient, 'fetchAccount').mockResolvedValue(account)
    const createSpy = spyOn(apiClient, 'createAccount').mockResolvedValue(
      account,
    )
    const updateSpy = spyOn(apiClient, 'updateAccount').mockResolvedValue(
      account,
    )
    const logoSpy = spyOn(apiClient, 'uploadAccountLogo').mockResolvedValue(
      account,
    )
    const verifySpy = spyOn(apiClient, 'verifyAccount').mockResolvedValue({
      ok: true,
      identityEmail: 'primary@example.com',
      pushSupported: false,
    })
    const enableSpy = spyOn(apiClient, 'enableAccount').mockResolvedValue({
      ok: true,
    })
    const disableSpy = spyOn(apiClient, 'disableAccount').mockResolvedValue({
      ok: true,
    })
    const deleteSpy = spyOn(apiClient, 'deleteAccount').mockResolvedValue({
      ok: true,
    })
    const oauthSpy = spyOn(apiClient, 'startProviderOAuth').mockResolvedValue({
      authorizationUrl: 'https://accounts.example.test/auth',
      state: 'state-1',
      redirectUri: 'http://localhost:3001/v1/oauth/callback',
    })

    try {
      const redirectUri = runtimeViews.oauth.redirectUri()
      const logo = new File(['logo'], 'logo.png', { type: 'image/png' })

      expect(redirectUri).toBe(apiClient.buildOAuthRedirectUri())
      expect(await runtimeViews.accounts.detail('primary')).toBe(account)
      expect(await runtimeMutations.accounts.create(createInput)).toBe(account)
      expect(
        await runtimeMutations.accounts.update('primary', { name: 'Renamed' }),
      ).toBe(account)
      expect(await runtimeMutations.accounts.uploadLogo('primary', logo)).toBe(
        account,
      )
      await expect(
        runtimeMutations.accounts.verify('primary'),
      ).resolves.toEqual({
        ok: true,
        identityEmail: 'primary@example.com',
        pushSupported: false,
      })
      await expect(
        runtimeMutations.accounts.enable('primary'),
      ).resolves.toEqual({
        ok: true,
      })
      await expect(
        runtimeMutations.accounts.disable('primary'),
      ).resolves.toEqual({
        ok: true,
      })
      await expect(
        runtimeMutations.accounts.delete('primary'),
      ).resolves.toEqual({
        ok: true,
      })
      await expect(
        runtimeMutations.oauth.startProvider({
          provider: 'gmail',
          clientId: 'client-1',
          redirectUri,
        }),
      ).resolves.toEqual({
        authorizationUrl: 'https://accounts.example.test/auth',
        state: 'state-1',
        redirectUri: 'http://localhost:3001/v1/oauth/callback',
      })
      expect(fetchSpy).toHaveBeenCalledWith('primary')
      expect(createSpy).toHaveBeenCalledWith(createInput)
      expect(updateSpy).toHaveBeenCalledWith('primary', { name: 'Renamed' })
      expect(logoSpy).toHaveBeenCalledWith('primary', logo)
      expect(verifySpy).toHaveBeenCalledWith('primary')
      expect(enableSpy).toHaveBeenCalledWith('primary')
      expect(disableSpy).toHaveBeenCalledWith('primary')
      expect(deleteSpy).toHaveBeenCalledWith('primary')
      expect(oauthSpy).toHaveBeenCalledWith({
        provider: 'gmail',
        clientId: 'client-1',
        redirectUri,
      })
    } finally {
      fetchSpy.mockRestore()
      createSpy.mockRestore()
      updateSpy.mockRestore()
      logoSpy.mockRestore()
      verifySpy.mockRestore()
      enableSpy.mockRestore()
      disableSpy.mockRestore()
      deleteSpy.mockRestore()
      oauthSpy.mockRestore()
    }
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
      expect(await runtimeViews.accounts.list()).toEqual([account])
      expect(
        await runtimeMutations.accounts.sync({
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
