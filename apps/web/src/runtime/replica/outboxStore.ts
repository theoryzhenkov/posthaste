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
 * @spec docs/replication/L3#4-indexeddb-persistence
 */
import type { ReplicaAssertion } from './handle'

export interface OutboxRecord {
  clientMutationId: string
  messageId: string
  assertion: ReplicaAssertion
  runtimeMutationId: string | null
  acceptedAt: number
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
  ): Promise<void>
  remove(clientMutationId: string): Promise<void>
  all(): Promise<OutboxRecord[]>
}

const DB_NAME = 'posthaste-replica'
const STORE_NAME = 'outbox'
const VERSION = 1

function openConnection(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(DB_NAME, VERSION)
    request.onupgradeneeded = () => {
      const connection = request.result
      if (!connection.objectStoreNames.contains(STORE_NAME)) {
        connection.createObjectStore(STORE_NAME, {
          keyPath: 'clientMutationId',
        })
      }
    }
    request.onsuccess = () => resolve(request.result)
    request.onerror = () => reject(request.error)
  })
}

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
    this.connection ??= openConnection()
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
        store.put({ ...existing, runtimeMutationId })
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
  ): Promise<void> {
    const existing = this.records.get(clientMutationId)
    if (existing) {
      existing.runtimeMutationId = runtimeMutationId
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
