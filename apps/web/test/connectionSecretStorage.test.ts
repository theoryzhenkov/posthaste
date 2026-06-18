import { afterEach, describe, expect, it } from 'bun:test'

import type { ConnectionsFile } from '../src/connection/types'
import {
  clientStore,
  defaultConnectionsFile,
  resetClientStoreForTesting,
} from '../src/connection/store'
import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

function localStorageSnapshot(): Record<string, string> {
  const snapshot: Record<string, string> = {}
  for (let index = 0; index < window.localStorage.length; index += 1) {
    const key = window.localStorage.key(index)
    if (!key) continue
    snapshot[key] = window.localStorage.getItem(key) ?? ''
  }
  return snapshot
}

afterEach(() => {
  resetClientStoreForTesting()
  window.localStorage.clear()
})

describe('connection secret storage', () => {
  it('web client store never persists profile tokens in localStorage', async () => {
    const store = clientStore()
    expect(store.supportsSecureTokens).toBe(false)

    await store.setToken('remote-1', 'remote-secret-token')
    await store.saveConnections({
      version: 1,
      activeProfileId: 'remote-1',
      profiles: [
        {
          id: 'remote-1',
          name: 'Remote daemon',
          mode: 'remote',
          baseUrl: 'https://daemon.example.test/v1',
          hostHeader: 'daemon.example.test',
          tokenRef: 'remote-1',
        },
      ],
    })

    expect(await store.getToken('remote-1')).toBeUndefined()
    expect(JSON.stringify(localStorageSnapshot())).not.toContain(
      'remote-secret-token',
    )
  })

  it('rejects inline secret fields in connection-profile storage', async () => {
    for (const secretField of [
      'token',
      'authToken',
      'accessToken',
      'bearerToken',
      'secret',
      'password',
      'credential',
      'authorization',
      'authHeader',
      'bearer',
      'apiKey',
      'api_key',
      'api-key',
      'privateKey',
      'private_key',
      'private-key',
      'auth_header',
      'auth-header',
    ]) {
      resetClientStoreForTesting()
      window.localStorage.clear()
      window.localStorage.setItem(
        'posthaste-connections-v1',
        JSON.stringify({
          version: 1,
          activeProfileId: 'remote-1',
          profiles: [
            {
              id: 'remote-1',
              name: 'Remote daemon',
              mode: 'remote',
              baseUrl: 'https://daemon.example.test/v1',
              [secretField]: 'must-not-be-trusted',
            },
          ],
        }),
      )

      const loaded = await clientStore().loadConnections()
      expect(loaded).toEqual(defaultConnectionsFile())
      expect(JSON.stringify(loaded)).not.toContain('must-not-be-trusted')
    }
  })

  it('rejects top-level connection storage fields that could carry secrets', async () => {
    for (const extraField of [
      { token: 'must-not-be-trusted' },
      { secret: 'must-not-be-trusted' },
      { metadata: { authorization: 'Bearer must-not-be-trusted' } },
    ]) {
      resetClientStoreForTesting()
      window.localStorage.clear()
      window.localStorage.setItem(
        'posthaste-connections-v1',
        JSON.stringify({
          version: 1,
          activeProfileId: 'embedded',
          profiles: [
            { id: 'embedded', name: 'This computer', mode: 'embedded' },
          ],
          ...extraField,
        }),
      )

      const loaded = await clientStore().loadConnections()
      expect(loaded).toEqual(defaultConnectionsFile())
      expect(JSON.stringify(loaded)).not.toContain('must-not-be-trusted')
    }
  })

  it('rejects unknown nested connection-profile fields that could carry secrets', async () => {
    for (const extraField of [
      { metadata: { token: 'must-not-be-trusted' } },
      { headers: { Authorization: 'Bearer must-not-be-trusted' } },
    ]) {
      resetClientStoreForTesting()
      window.localStorage.clear()
      window.localStorage.setItem(
        'posthaste-connections-v1',
        JSON.stringify({
          version: 1,
          activeProfileId: 'remote-1',
          profiles: [
            {
              id: 'remote-1',
              name: 'Remote daemon',
              mode: 'remote',
              baseUrl: 'https://daemon.example.test/v1',
              ...extraField,
            },
          ],
        }),
      )

      const loaded = await clientStore().loadConnections()
      expect(loaded).toEqual(defaultConnectionsFile())
      expect(JSON.stringify(loaded)).not.toContain('must-not-be-trusted')
    }
  })

  it('rejects URL-carried secrets in connection-profile base URLs', async () => {
    for (const baseUrl of [
      'https://user:password@daemon.example.test/v1',
      'https://daemon.example.test/v1?access_token=must-not-be-trusted',
      'https://daemon.example.test/v1?token=must-not-be-trusted',
      'https://daemon.example.test/v1#must-not-be-trusted',
    ]) {
      resetClientStoreForTesting()
      window.localStorage.clear()
      window.localStorage.setItem(
        'posthaste-connections-v1',
        JSON.stringify({
          version: 1,
          activeProfileId: 'remote-1',
          profiles: [
            {
              id: 'remote-1',
              name: 'Remote daemon',
              mode: 'remote',
              baseUrl,
            },
          ],
        }),
      )

      const loaded = await clientStore().loadConnections()
      expect(loaded).toEqual(defaultConnectionsFile())
      expect(JSON.stringify(loaded)).not.toContain('must-not-be-trusted')
    }
  })

  it('refuses to save unsafe connection-profile base URLs', async () => {
    await expect(
      clientStore().saveConnections({
        version: 1,
        activeProfileId: 'remote-1',
        profiles: [
          {
            id: 'remote-1',
            name: 'Remote daemon',
            mode: 'remote',
            baseUrl:
              'https://daemon.example.test/v1?access_token=must-not-be-trusted',
          },
        ],
      }),
    ).rejects.toThrow('connection profile store contains unsafe fields')
    expect(JSON.stringify(localStorageSnapshot())).not.toContain(
      'must-not-be-trusted',
    )
  })

  it('serialized connection profiles carry token refs, not token values', async () => {
    const file: ConnectionsFile = {
      version: 1,
      activeProfileId: 'remote-1',
      profiles: [
        {
          id: 'remote-1',
          name: 'Remote daemon',
          mode: 'remote',
          baseUrl: 'https://daemon.example.test/v1',
          tokenRef: 'remote-token-entry',
        },
      ],
    }

    await clientStore().saveConnections(file)
    const snapshot = JSON.stringify(localStorageSnapshot())

    expect(snapshot).toContain('remote-token-entry')
    expect(snapshot).not.toContain('Authorization')
    expect(snapshot).not.toContain('Bearer')
    expect(snapshot).not.toContain('remote-secret-token')
  })
})
