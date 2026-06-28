/**
 * Shared IndexedDB schema for the replica's durable state. The outbox + undo
 * history live in one database (`posthaste-replica`) and upgrade together:
 * whichever store opens first runs `onupgradeneeded` and creates BOTH object
 * stores. This prevents the version-drift regression where one store bumped the
 * shared DB past the other — a downgrade `open(..., olderVersion)` throws
 * `VersionError`, which surfaced as views stuck loading forever (the outbox
 * rehydration in `openMailListView` rejected + was swallowed).
 *
 * Add new replica object stores here, in the shared `onupgradeneeded`, and bump
 * `REPLICA_DB_VERSION`. Never open `posthaste-replica` at a different version
 * elsewhere.
 *
 * @spec docs/replication/client-link/L3#3-indexeddb-persistence
 */
export const REPLICA_DB_NAME = 'posthaste-replica'
export const REPLICA_DB_VERSION = 2
export const OUTBOX_STORE = 'outbox'
export const UNDO_HISTORY_STORE = 'undoHistory'

/**
 * Open the shared replica database at the current schema version, creating both
 * object stores on first open / upgrade. Callers cache + reuse the connection.
 */
export function openReplicaDatabase(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(REPLICA_DB_NAME, REPLICA_DB_VERSION)
    request.onupgradeneeded = () => {
      const connection = request.result
      if (!connection.objectStoreNames.contains(OUTBOX_STORE)) {
        connection.createObjectStore(OUTBOX_STORE, {
          keyPath: 'clientMutationId',
        })
      }
      if (!connection.objectStoreNames.contains(UNDO_HISTORY_STORE)) {
        connection.createObjectStore(UNDO_HISTORY_STORE, { keyPath: 'key' })
      }
    }
    request.onsuccess = () => resolve(request.result)
    request.onerror = () => reject(request.error)
  })
}
