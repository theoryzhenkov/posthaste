/**
 * Shared IndexedDB schema for the replica's durable state. The pending set +
 * undo history live in one database (`posthaste-replica`) and upgrade
 * together: whichever store opens first runs `onupgradeneeded` and creates
 * BOTH object stores. This prevents the version-drift regression where one
 * store bumped the shared DB past the other — a downgrade
 * `open(..., olderVersion)` throws `VersionError`, which surfaced as views
 * stuck loading forever (the pending-set rehydration in `openMailListView`
 * rejected + was swallowed).
 *
 * Add new replica object stores here, in the shared `onupgradeneeded`, and bump
 * `REPLICA_DB_VERSION`. Never open `posthaste-replica` at a different version
 * elsewhere.
 *
 * @spec docs/replication/client-link/L3#3-indexeddb-persistence
 */
export const REPLICA_DB_NAME = 'posthaste-replica'
export const REPLICA_DB_VERSION = 2
// D54 renamed the in-code pending-mutation-set vocabulary from "outbox" to
// "PendingSet" (see `pendingSetStore.ts`), but this object-store name is
// on-disk data for every existing install — renaming the persisted string
// would need a migration, which is out of scope here. Keep the literal.
export const OUTBOX_STORE = 'outbox'
export const UNDO_HISTORY_STORE = 'undoHistory'

/**
 * Every live connection this module has handed out. `resetReplicaDatabase` must
 * close them all before `deleteDatabase`, which otherwise blocks indefinitely
 * while any connection is open.
 */
const openConnections = new Set<IDBDatabase>()

/**
 * Multi-tab schema-version notices (W1 / N19):
 *
 * - `'blocked'`: this tab's `open()` is waiting on another connection (this
 *   tab or another tab) that hasn't closed for the upgrade yet. The open
 *   request is NOT rejected — the browser still resolves it once the blocker
 *   closes (normally via its own `'outdated'`-triggering `onversionchange`
 *   below) — but a stuck blocker (e.g. a background tab with no handler, or a
 *   long-running transaction) can leave this pending far longer than a user
 *   would tolerate, so callers use this to nudge them (e.g. "reload other
 *   tabs").
 * - `'outdated'`: this tab's own connection was just closed because another
 *   tab/context is upgrading to a newer schema version. Every store built on
 *   {@link openReplicaDatabase} stops working after this fires (its cached
 *   connection is closed) — callers use this to prompt a reload.
 *
 * Kept UI-agnostic on purpose (no `sonner`/toast import here): the app shell
 * subscribes and decides how to surface it.
 */
export type ReplicaDatabaseNotice = 'blocked' | 'outdated'
type ReplicaDatabaseNoticeListener = (notice: ReplicaDatabaseNotice) => void

const noticeListeners = new Set<ReplicaDatabaseNoticeListener>()

export function onReplicaDatabaseNotice(
  listener: ReplicaDatabaseNoticeListener,
): () => void {
  noticeListeners.add(listener)
  return () => noticeListeners.delete(listener)
}

function emitReplicaDatabaseNotice(notice: ReplicaDatabaseNotice): void {
  for (const listener of noticeListeners) {
    listener(notice)
  }
}

/**
 * Open the shared replica database at the current schema version, creating both
 * object stores on first open / upgrade. Callers cache + reuse the connection.
 *
 * Multi-tab safe (W1 / N19): without the handlers below, a second tab opening
 * a newer schema version (e.g. after a deploy) deadlocks BOTH tabs — the
 * older tab never releases its connection, so the newer tab's `open()` blocks
 * forever and its views spin loading indefinitely. `onversionchange` makes
 * every connection close itself proactively the moment another context wants
 * to upgrade past it, which is what lets the newer tab's `open()` proceed;
 * `onblocked` is the backstop notice for the (rarer) case where a blocker
 * hasn't closed yet — e.g. mid-transaction, or a stale tab that predates this
 * fix.
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
    request.onblocked = () => emitReplicaDatabaseNotice('blocked')
    request.onsuccess = () => {
      const connection = request.result
      openConnections.add(connection)
      // A version-change (e.g. a reset's deleteDatabase) closes the connection;
      // drop it so the tracking set doesn't leak stale handles.
      connection.addEventListener('close', () =>
        openConnections.delete(connection),
      )
      // THE deadlock fix: a newer tab/context wants to upgrade past this
      // connection's version. Close proactively instead of holding the DB
      // open indefinitely — without this, the newer tab's `open()` blocks
      // forever (the deploy-time "views stuck loading" regression).
      connection.onversionchange = () => {
        connection.close()
        emitReplicaDatabaseNotice('outdated')
      }
      resolve(connection)
    }
    request.onerror = () => reject(request.error)
  })
}

/**
 * Delete the entire replica database (pending set + undo history) — the
 * client-side store that the reactive mail-list views are computed from. This
 * is the missing half of "repair": rebuilding the server-side `mail.sqlite`
 * leaves a wedged replica untouched, which is the real cause of "views stuck
 * loading forever".
 *
 * The replica is a rebuildable cache: on the next open it re-hydrates from the
 * runtime/server. The only data lost is never-dispatched pending-set
 * mutations (the caller must warn the user). Intended to be followed by a
 * relaunch / re-init.
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
