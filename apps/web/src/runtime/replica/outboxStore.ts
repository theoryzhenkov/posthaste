/**
 * Durable home of the replica's unconfirmed intent. The outbox is the one piece
 * of replica state that must survive a reload (losing it loses user mutations),
 * so it persists in IndexedDB keyed by the client mutation id; the served base
 * is rebuilt from the runtime snapshot and is never stored here.
 *
 * Records hold only mutation metadata (target message id, keyword/mailbox
 * assertion, the id pairing) — no message body, attachment, or auth material,
 * which is why this module is the sole sanctioned IndexedDB user
 * (`rendererStorageBoundary` allow-list).
 *
 * @spec docs/replication/client-link/L3#3-indexeddb-persistence
 */
import type { ReplicaAssertion } from './handle'
import type { RuntimeRunMutationRequest } from '../types'
import { openReplicaDatabase, OUTBOX_STORE } from './replicaDatabase'

export interface OutboxRecord {
  clientMutationId: string
  messageId: string
  assertion: ReplicaAssertion
  runtimeMutationId: string | null
  acceptedAt: number
  /**
   * The original runtime send, stored so a never-dispatched record
   * (`runtimeMutationId === null`) can be replayed verbatim by the near-end
   * engine's reconciler on connect. Optional: records written before this
   * field existed lack it and are skipped on replay (can't reconstruct the
   * send). IndexedDB is schemaless, so adding this needs NO `replicaDatabase`
   * version bump.
   */
  request?: RuntimeRunMutationRequest
  /**
   * The runtime link the record was dispatched under (stored at link
   * time). The reconciler's sent-but-unsettled settlement query (D44b) is
   * keyed to it; a legacy record without one cannot be queried and is dropped
   * on rehydration (the pre-reconciler behavior). Schemaless: no version bump.
   */
  linkId?: string
}

/**
 * The persistence seam the replicaAdapter drives. Implementations are keyed by
 * `clientMutationId` and idempotent on `put` (re-accept is a no-op overwrite).
 */
export interface OutboxStore {
  put(record: OutboxRecord): Promise<void>
  linkRuntimeMutationId(
    clientMutationId: string,
    runtimeMutationId: string,
    linkId?: string,
  ): Promise<void>
  remove(clientMutationId: string): Promise<void>
  all(): Promise<OutboxRecord[]>
}

const STORE_NAME = OUTBOX_STORE

function runRequest<T>(request: IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    request.onsuccess = () => resolve(request.result)
    request.onerror = () => reject(request.error)
  })
}

/**
 * IndexedDB-backed outbox. The connection opens lazily and is reused; a fresh
 * store is created on first open and migrated by version bump (records are
 * never silently dropped — they are the only home of unconfirmed intent).
 */
export class IndexedDbOutboxStore implements OutboxStore {
  private connection: Promise<IDBDatabase> | undefined

  private db(): Promise<IDBDatabase> {
    this.connection ??= openReplicaDatabase()
    return this.connection
  }

  private async write(mutate: (store: IDBObjectStore) => void): Promise<void> {
    const connection = await this.db()
    await new Promise<void>((resolve, reject) => {
      const transaction = connection.transaction(STORE_NAME, 'readwrite')
      transaction.oncomplete = () => resolve()
      transaction.onerror = () => reject(transaction.error)
      transaction.onabort = () => reject(transaction.error)
      mutate(transaction.objectStore(STORE_NAME))
    })
  }

  async put(record: OutboxRecord): Promise<void> {
    await this.write((store) => store.put(record))
  }

  async linkRuntimeMutationId(
    clientMutationId: string,
    runtimeMutationId: string,
    linkId?: string,
  ): Promise<void> {
    const connection = await this.db()
    await new Promise<void>((resolve, reject) => {
      const transaction = connection.transaction(STORE_NAME, 'readwrite')
      transaction.onerror = () => reject(transaction.error)
      transaction.onabort = () => reject(transaction.error)
      const store = transaction.objectStore(STORE_NAME)
      const get = store.get(clientMutationId)
      get.onsuccess = () => {
        const existing = get.result as OutboxRecord | undefined
        if (!existing) {
          resolve()
          return
        }
        store.put({
          ...existing,
          runtimeMutationId,
          ...(linkId ? { linkId } : {}),
        })
        transaction.oncomplete = () => resolve()
      }
      get.onerror = () => reject(get.error)
    })
  }

  async remove(clientMutationId: string): Promise<void> {
    await this.write((store) => store.delete(clientMutationId))
  }

  async all(): Promise<OutboxRecord[]> {
    const connection = await this.db()
    const transaction = connection.transaction(STORE_NAME, 'readonly')
    const records = await runRequest(
      transaction.objectStore(STORE_NAME).getAll() as IDBRequest<
        OutboxRecord[]
      >,
    )
    return records.sort((a, b) => a.acceptedAt - b.acceptedAt)
  }
}

/**
 * In-memory outbox for tests and SSR/no-IndexedDB environments. Same semantics,
 * no durability.
 */
export class MemoryOutboxStore implements OutboxStore {
  private readonly records = new Map<string, OutboxRecord>()

  async put(record: OutboxRecord): Promise<void> {
    this.records.set(record.clientMutationId, { ...record })
  }

  async linkRuntimeMutationId(
    clientMutationId: string,
    runtimeMutationId: string,
    linkId?: string,
  ): Promise<void> {
    const existing = this.records.get(clientMutationId)
    if (existing) {
      existing.runtimeMutationId = runtimeMutationId
      if (linkId) {
        existing.linkId = linkId
      }
    }
  }

  async remove(clientMutationId: string): Promise<void> {
    this.records.delete(clientMutationId)
  }

  async all(): Promise<OutboxRecord[]> {
    return [...this.records.values()].sort(
      (a, b) => a.acceptedAt - b.acceptedAt,
    )
  }
}

/**
 * The default outbox for the current environment: durable IndexedDB when
 * available, otherwise an in-memory fallback.
 */
export function defaultOutboxStore(): OutboxStore {
  if (typeof indexedDB !== 'undefined') {
    return new IndexedDbOutboxStore()
  }
  return new MemoryOutboxStore()
}
