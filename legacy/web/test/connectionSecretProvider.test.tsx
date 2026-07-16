import { afterEach, describe, expect, it } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'

import { useActiveConnection } from '../src/connection/connectionContext'
import type { ClientStore } from '../src/connection/store'
import {
  defaultConnectionsFile,
  resetClientStoreForTesting,
} from '../src/connection/store'
import type { ConnectionsFile } from '../src/connection/types'
import { ActiveConnectionProvider } from '../src/connection/useActiveConnection'
import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

class RecordingClientStore implements ClientStore {
  readonly supportsSecureTokens = true
  readonly savedFiles: ConnectionsFile[] = []
  readonly setTokenCalls: Array<{ profileId: string; token: string }> = []
  readonly deletedTokenIds: string[] = []
  file: ConnectionsFile = defaultConnectionsFile()

  async loadConnections(): Promise<ConnectionsFile> {
    return this.file
  }

  async saveConnections(file: ConnectionsFile): Promise<void> {
    this.savedFiles.push(file)
    this.file = file
  }

  async getToken(): Promise<string | undefined> {
    return undefined
  }

  async setToken(profileId: string, token: string): Promise<void> {
    this.setTokenCalls.push({ profileId, token })
  }

  async deleteToken(profileId: string): Promise<void> {
    this.deletedTokenIds.push(profileId)
  }
}

function createWrapper(): (props: { children: ReactNode }) => ReactNode {
  const queryClient = new QueryClient()
  return function Wrapper({ children }: { children: ReactNode }): ReactNode {
    return (
      <QueryClientProvider client={queryClient}>
        <ActiveConnectionProvider>{children}</ActiveConnectionProvider>
      </QueryClientProvider>
    )
  }
}

afterEach(() => {
  resetClientStoreForTesting()
})

describe('active connection secret storage', () => {
  it('validates remote profiles before saving tokens or updating memory', async () => {
    const store = new RecordingClientStore()
    resetClientStoreForTesting(store)

    const { result } = renderHook(() => useActiveConnection(), {
      wrapper: createWrapper(),
    })
    await waitFor(() => expect(result.current.profiles).toHaveLength(1))

    let thrown: unknown
    try {
      await act(async () => {
        await result.current.addProfile({
          mode: 'remote',
          name: 'Unsafe daemon',
          baseUrl:
            'https://daemon.example.test/v1?access_token=must-not-be-trusted',
          token: 'remote-secret-token',
        })
      })
    } catch (error) {
      thrown = error
    }

    expect(thrown).toBeInstanceOf(Error)
    expect((thrown as Error).message).toBe(
      'connection profile store contains unsafe fields',
    )
    expect(store.setTokenCalls).toEqual([])
    expect(store.savedFiles).toEqual([])
    expect(result.current.profiles).toEqual(defaultConnectionsFile().profiles)
  })
})
