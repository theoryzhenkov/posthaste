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
 * Every live connection this module has handed out. `resetReplicaDatabase` must
 * close them all before `deleteDatabase`, which otherwise blocks indefinitely
 * while any connection is open.
 */
const openConnections = new Set<IDBDatabase>()

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
    request.onsuccess = () => {
      const connection = request.result
      openConnections.add(connection)
      // A version-change (e.g. a reset's deleteDatabase) closes the connection;
      // drop it so the tracking set doesn't leak stale handles.
      connection.addEventListener('close', () =>
        openConnections.delete(connection),
      )
      resolve(connection)
    }
    request.onerror = () => reject(request.error)
  })
}

/**
 * Delete the entire replica database (outbox + undo history) — the client-side
 * store that the reactive mail-list views are computed from. This is the missing
 * half of "repair": rebuilding the server-side `mail.sqlite` leaves a wedged
 * replica untouched, which is the real cause of "views stuck loading forever".
 *
 * The replica is a rebuildable cache: on the next open it re-hydrates from the
 * runtime/server. The only data lost is never-dispatched outbox mutations (the
 * caller must warn the user). Intended to be followed by a relaunch / re-init.
 *
 * Closes every tracked connection first (an open connection blocks the delete);
 * callers that hold a cached connection must drop it after this resolves.
 */
export function resetReplicaDatabase(): Promise<void> {
  for (const connection of openConnections) {
    connection.close()
  }
  openConnections.clear()
  return new Promise((resolve, reject) => {
    const request = indexedDB.deleteDatabase(REPLICA_DB_NAME)
    request.onsuccess = () => resolve()
    request.onerror = () => reject(request.error)
    // A connection we don't track is still open: the delete is queued and
    // completes once it closes (e.g. on the relaunch that follows). Resolve so
    // the repair flow isn't wedged waiting on a straggler.
    request.onblocked = () => resolve()
  })
}
