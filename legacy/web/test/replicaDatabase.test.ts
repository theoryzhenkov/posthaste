/**
 * Multi-tab IndexedDB schema-version safety (W1 / N19). Drives the real
 * `openReplicaDatabase` open path against `fake-indexeddb` — a spec-compliant
 * in-memory IndexedDB — so `onversionchange`/`onblocked` are exercised for
 * real rather than mocked.
 */
import { afterEach, beforeEach, describe, expect, it } from 'bun:test'
import 'fake-indexeddb/auto'
import { IDBFactory } from 'fake-indexeddb'

import {
  onReplicaDatabaseNotice,
  openReplicaDatabase,
  OUTBOX_STORE,
  REPLICA_DB_NAME,
  REPLICA_DB_VERSION,
} from '../src/runtime/replica/replicaDatabase'

// Each test gets a fresh, empty origin — the module-level `openConnections`
// set in `replicaDatabase.ts` is otherwise shared across tests via the same
// DB name.
beforeEach(() => {
  globalThis.indexedDB = new IDBFactory() as unknown as typeof indexedDB
})

let unsubscribes: (() => void)[] = []
afterEach(() => {
  for (const unsubscribe of unsubscribes) {
    unsubscribe()
  }
  unsubscribes = []
})

function trackNotices(): string[] {
  const notices: string[] = []
  unsubscribes.push(onReplicaDatabaseNotice((notice) => notices.push(notice)))
  return notices
}

describe('openReplicaDatabase multi-tab safety', () => {
  it('closes the connection when another context wants to upgrade past it (the deadlock fix)', async () => {
    const notices = trackNotices()
    const connection = await openReplicaDatabase()
    expect(connection.version).toBe(REPLICA_DB_VERSION)

    // Simulate a second tab that loaded a newer build: it opens at a higher
    // schema version, which fires `versionchange` on our still-open
    // connection. Without an `onversionchange` handler that closes it, this
    // second open blocks forever — the exact deadlock W1 fixes. If the
    // `await` below ever hangs, the fix has regressed.
    const upgrade = indexedDB.open(REPLICA_DB_NAME, REPLICA_DB_VERSION + 1)
    await new Promise<void>((resolve, reject) => {
      upgrade.onupgradeneeded = () => resolve()
      upgrade.onsuccess = () => resolve()
      upgrade.onerror = () => reject(upgrade.error)
    })

    // `close()` (unlike a browser-forced close) does not fire the 'close'
    // event by spec — the observable proof our `onversionchange` handler ran
    // is (a) the upgrade above actually completed instead of hanging behind
    // `onblocked`, (b) the notice fired, and (c) the connection now rejects
    // new transactions.
    expect(notices).toContain('outdated')
    expect(() => connection.transaction(OUTBOX_STORE)).toThrow()
  })

  it('emits a "blocked" notice when a stale connection (no versionchange handler) holds up the upgrade, and still resolves once it closes', async () => {
    const notices = trackNotices()

    // A raw connection opened directly against `indexedDB` (bypassing
    // `openReplicaDatabase`) has no `onversionchange` handler — the
    // pre-W1 behavior that used to deadlock a second tab.
    const staleRequest = indexedDB.open(REPLICA_DB_NAME, 1)
    const stale = await new Promise<IDBDatabase>((resolve, reject) => {
      staleRequest.onupgradeneeded = () => {
        staleRequest.result.createObjectStore(OUTBOX_STORE, {
          keyPath: 'clientMutationId',
        })
      }
      staleRequest.onsuccess = () => resolve(staleRequest.result)
      staleRequest.onerror = () => reject(staleRequest.error)
    })

    const openPromise = openReplicaDatabase()
    // Give the open a turn to fire `onblocked` against the stale connection.
    await new Promise((resolve) => setTimeout(resolve, 10))
    expect(notices).toContain('blocked')

    // The stale tab closes (e.g. the user reloaded it) — the blocked open
    // now proceeds instead of hanging forever.
    stale.close()
    const connection = await openPromise
    expect(connection.version).toBe(REPLICA_DB_VERSION)
  })
})
