// React bindings for the MailClient facade: a context provider plus the live
// query hooks. A hook declares a query and renders its latest answer as
// { data, generation, status }; mounting joins the shared mirror entry,
// unmounting releases it. Liveness — refetch on stream generations, staleness
// on disconnect — is the facade's job, not the component's.

import {
  createContext,
  useCallback,
  useContext,
  useSyncExternalStore,
  type ReactNode,
} from 'react'
import {
  canonicalQueryKey,
  MailClient,
  type ConnectionStatus,
  type LiveResult,
} from './client'
import type {
  AccountId,
  AccountsResult,
  MailboxCountsResult,
  MailListQuery,
  MailListResult,
  MessageDetailQuery,
  MessageDetailResult,
  PendingOperationsResult,
  Query,
  ThreadQuery,
  ThreadView,
} from './gen'

const MailClientContext = createContext<MailClient | null>(null)

export function MailClientProvider({
  client,
  children,
}: {
  client: MailClient
  children: ReactNode
}) {
  return <MailClientContext.Provider value={client}>{children}</MailClientContext.Provider>
}

export function useMailClient(): MailClient {
  const client = useContext(MailClientContext)
  if (!client) throw new Error('useMailClient requires a <MailClientProvider> ancestor')
  return client
}

/** The one live-query primitive: retains the query for the lifetime of the
 * subscription and renders the mirror entry's snapshot. Queries with the same
 * canonical key share one entry and one fetch. */
export function useLiveQuery<T>(query: Query): LiveResult<T> {
  const client = useMailClient()
  const key = canonicalQueryKey(query)
  const subscribe = useCallback(
    (onChange: () => void) => {
      const retainedKey = client.retain(query)
      const unsubscribe = client.subscribeQuery(retainedKey, onChange)
      return () => {
        unsubscribe()
        client.release(retainedKey)
      }
    },
    // The query object is identified by its canonical key; a new object with
    // the same key must not resubscribe.
    [client, key],
  )
  const getSnapshot = useCallback(() => client.getSnapshot<T>(key), [client, key])
  return useSyncExternalStore(subscribe, getSnapshot)
}

/** A windowed mail list; the empty scope is "all mail, date descending". */
export function useMailList(scope: MailListQuery = {}): LiveResult<MailListResult> {
  return useLiveQuery<MailListResult>({ mailList: scope })
}

export function useThread(id: ThreadQuery): LiveResult<ThreadView> {
  return useLiveQuery<ThreadView>({ thread: id })
}

export function useMessage(id: MessageDetailQuery): LiveResult<MessageDetailResult> {
  return useLiveQuery<MessageDetailResult>({ messageDetail: id })
}

export function useMailboxCounts(accountId?: AccountId): LiveResult<MailboxCountsResult> {
  return useLiveQuery<MailboxCountsResult>({ mailboxCounts: { accountId } })
}

export function useAccounts(): LiveResult<AccountsResult> {
  return useLiveQuery<AccountsResult>({ accounts: {} })
}

export function usePendingOperations(accountId?: AccountId): LiveResult<PendingOperationsResult> {
  return useLiveQuery<PendingOperationsResult>({ pendingOperations: { accountId } })
}

/** The facade's connection state: while the backend is unreachable the last
 * answers stay rendered, marked stale, and reconnect refetches everything. */
export function useConnectionStatus(): ConnectionStatus {
  const client = useMailClient()
  const getStatus = useCallback(() => client.getConnectionStatus(), [client])
  return useSyncExternalStore(client.subscribeConnection, getStatus)
}
