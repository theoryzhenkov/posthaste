// Connection state as the facade reports it: while the backend is
// unreachable the stream reconnects and held answers stay rendered (marked
// stale); when it comes back, stream.ts invalidates everything mounted.

import { useCallback, useSyncExternalStore } from 'react'
import type { ConnectionStatus } from '@/domain/vocabulary'
import { useMailClient } from '../context'

export function useConnectionStatus(): ConnectionStatus {
  const client = useMailClient()
  const getStatus = useCallback(() => client.getConnectionStatus(), [client])
  return useSyncExternalStore(client.subscribeConnection, getStatus)
}
