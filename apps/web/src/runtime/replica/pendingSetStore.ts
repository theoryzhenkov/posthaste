/**
 * Durable home of the replica's unconfirmed intent. The pending set is the one
 * piece of replica state that must survive a reload (losing it loses user
 * mutations), so it persists in IndexedDB keyed by the client mutation id; the
 * served base is rebuilt from the runtime snapshot and is never stored here.
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

export interface PendingSetRecord {
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
export interface PendingSetStore {
  put(record: PendingSetRecord): Promise<void>
  linkRuntimeMutationId(
    clientMutationId: string,
    runtimeMutationId: string,
    linkId?: string,
  ): Promise<void>
  remove(clientMutationId: string): Promise<void>
  all(): Promise<PendingSetRecord[]>
}

// `OUTBOX_STORE` (D54): the persisted IndexedDB object-store name stays
// literally `'outbox'` — it is on-disk data for every existing install, and
// renaming it would require a client-side migration that is out of scope
// here. Only the module/type/identifier names above changed to `PendingSet*`;
// the wire-level store name did not.
const STORE_NAME = OUTBOX_STORE

function runRequest<T>(request: IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    request.onsuccess = () => resolve(request.result)
    request.onerror = () => reject(request.error)
  })
}

function isQuotaExceededError(error: unknown): boolean {
  return (
    error instanceof Error &&
    (error.name === 'QuotaExceededError' ||
      // Firefox's legacy DOMException code path.
      (error as { code?: number }).code === 22)
  )
}

/**
 * Thrown when a durable pending-set write fails (W4 / N21). Distinguishes
 * "the origin's IndexedDB storage is full" — the common, actionable case —
 * from any other transaction failure, so a caller can surface a clear message
 * ("storage full") instead of an opaque `DOMException`, and so the optimistic
 * store fold that preceded the write can be reverted rather than left
 * stranded (see `entityStoreAdapter.ts`'s `runMutation`). The underlying
 * IndexedDB transaction is atomic — a failed write is rolled back by the
 * browser itself, so the store is never left half-written; this type is about
 * making the failure VISIBLE, not about repairing corruption that can't occur.
 */
export class PendingSetWriteError extends Error {
  readonly quotaExceeded: boolean

  constructor(cause: unknown) {
    const quotaExceeded = isQuotaExceededError(cause)
    super(
      quotaExceeded
        ? 'Local storage is full — free up space and try again.'
        : `Pending-set write failed: ${
            cause instanceof Error ? cause.message : String(cause)
          }`,
    )
    this.name = 'PendingSetWriteError'
    this.quotaExceeded = quotaExceeded
    if (cause instanceof Error) {
      this.cause = cause
    }
  }
}

/**
 * Normalize any `PendingSetStore` write failure to a {@link PendingSetWriteError}
 * without double-wrapping one that's already classified. Used both inside
 * `IndexedDbPendingSetStore` and by callers (e.g. `entityStoreAdapter.ts`)
 * that want a uniform "storage full" message regardless of which
 * `PendingSetStore` implementation raised it (real IndexedDB or a test
 * double).
 */
export function toPendingSetWriteError(error: unknown): PendingSetWriteError {
  return error instanceof PendingSetWriteError
    ? error
    : new PendingSetWriteError(error)
}

/**
 * IndexedDB-backed pending set. The connection opens lazily and is reused; a
 * fresh store is created on first open and migrated by version bump (records
 * are never silently dropped — they are the only home of unconfirmed intent).
 */
export class IndexedDbPendingSetStore implements PendingSetStore {
  private connection: Promise<IDBDatabase> | undefined

  private db(): Promise<IDBDatabase> {
    this.connection ??= openReplicaDatabase()
    return this.connection
  }

  private async write(mutate: (store: IDBObjectStore) => void): Promise<void> {
    const connection = await this.db()
    try {
      await new Promise<void>((resolve, reject) => {
        const transaction = connection.transaction(STORE_NAME, 'readwrite')
        transaction.oncomplete = () => resolve()
        transaction.onerror = () => reject(transaction.error)
        transaction.onabort = () => reject(transaction.error)
        mutate(transaction.objectStore(STORE_NAME))
      })
    } catch (error) {
      // W4 / N21: a QuotaExceededError here is otherwise an opaque
      // DOMException that silently drops the durable write. Wrap it so
      // callers can tell "storage full" apart from other failures and react
      // (e.g. revert the optimistic fold that preceded this write).
      throw new PendingSetWriteError(error)
    }
  }

  async put(record: PendingSetRecord): Promise<void> {
    await this.write((store) => store.put(record))
  }

  async linkRuntimeMutationId(
    clientMutationId: string,
    runtimeMutationId: string,
    linkId?: string,
  ): Promise<void> {
    const connection = await this.db()
    try {
      await new Promise<void>((resolve, reject) => {
        const transaction = connection.transaction(STORE_NAME, 'readwrite')
        transaction.onerror = () => reject(transaction.error)
        transaction.onabort = () => reject(transaction.error)
        const store = transaction.objectStore(STORE_NAME)
        const get = store.get(clientMutationId)
        get.onsuccess = () => {
          const existing = get.result as PendingSetRecord | undefined
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
    } catch (error) {
      throw new PendingSetWriteError(error)
    }
  }

  async remove(clientMutationId: string): Promise<void> {
    await this.write((store) => store.delete(clientMutationId))
  }

  async all(): Promise<PendingSetRecord[]> {
    const connection = await this.db()
    const transaction = connection.transaction(STORE_NAME, 'readonly')
    const records = await runRequest(
      transaction.objectStore(STORE_NAME).getAll() as IDBRequest<
        PendingSetRecord[]
      >,
    )
    return records.sort((a, b) => a.acceptedAt - b.acceptedAt)
  }
}

/**
 * In-memory pending set for tests and SSR/no-IndexedDB environments. Same
 * semantics, no durability.
 */
export class MemoryPendingSetStore implements PendingSetStore {
  private readonly records = new Map<string, PendingSetRecord>()

  async put(record: PendingSetRecord): Promise<void> {
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

  async all(): Promise<PendingSetRecord[]> {
    return [...this.records.values()].sort(
      (a, b) => a.acceptedAt - b.acceptedAt,
    )
  }
}

/**
 * The default pending set for the current environment: durable IndexedDB when
 * available, otherwise an in-memory fallback.
 */
export function defaultPendingSetStore(): PendingSetStore {
  if (typeof indexedDB !== 'undefined') {
    return new IndexedDbPendingSetStore()
  }
  return new MemoryPendingSetStore()
}
