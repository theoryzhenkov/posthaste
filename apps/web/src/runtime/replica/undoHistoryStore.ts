/**
 * Durable home of the client-owned undo/redo history — Phase 1 of the synced-
 * history refactor. Holds an append-ordered `RevStep[]` + a `cursor` (the index
 * of the topmost APPLIED step; -1 = all undone). Navigation is LOCAL: chained
 * undo pops the cursor in memory and returns each step to invert, so N undos do
 * not cost N round trips. The diff is captured client-side
 * (`captureMutationDiffJson`); the runtime's per-session seq-keyed stacks are
 * retired. Persisted alongside the outbox (IndexedDB) so it survives reload.
 *
 * Shape is Phase-2-ready: steps carry stable `id`s (not session seqs) + the
 * cursor moves are idempotent id-keyed assignments, so promoting the log to a
 * server-authoritative synced view is additive, not a rewrite.
 *
 * @spec docs/eph/DESIGN-L2-undo-redo-synced-history
 */
import type { MessageChangeDiff } from './handle'

/** One recorded reversible step on the history. */
export interface RevStep {
  /** Stable, globally-orderable id (not a session seq) — the Phase 2 cursor key. */
  id: string
  messageId: string
  sourceId: string
  diff: MessageChangeDiff
  createdAt: number
}

/**
 * The full history state: `steps[0..cursor]` are applied (undoable, newest
 * first), `steps[cursor+1..]` are undone (redoable, oldest first). `cursor` is
 * -1 when everything has been undone.
 */
export interface UndoHistorySnapshot {
  steps: RevStep[]
  cursor: number
}

/** The store seam the adapter (forward actions) + hook (undo/redo) drive. */
export interface UndoHistoryStore {
  /** Load the persisted snapshot (once); subsequent calls return the cache. */
  load(): Promise<UndoHistorySnapshot>
  /** The cached snapshot (sync read for the hook's canUndo/canRedo). */
  snapshot(): UndoHistorySnapshot
  /** Record a forward action: truncate the redoable tail, append, advance. */
  pushForward(step: RevStep): Promise<UndoHistorySnapshot>
  /** Undo: decrement the cursor, return the step whose inverse to apply. */
  navigateUndo(): Promise<RevStep | null>
  /** Redo: increment the cursor, return the step to re-apply. */
  navigateRedo(): Promise<RevStep | null>
  /** Empty the history. */
  clear(): Promise<void>
  /** Notify on every history change (the hook keeps canUndo/canRedo fresh). */
  subscribe(listener: (snapshot: UndoHistorySnapshot) => void): () => void
}

/// Upper bound on retained history (matches the runtime's former `MAX_HISTORY`).
const MAX_HISTORY = 50

/** Truncate the redoable tail, append the step, advance the cursor to it. */
function applyPushForward(
  state: UndoHistorySnapshot,
  step: RevStep,
): UndoHistorySnapshot {
  const steps = state.steps.slice(0, state.cursor + 1)
  steps.push(step)
  return { steps, cursor: steps.length - 1 }
}

/** Decrement the cursor; return the step to invert (or null at the bottom). */
function applyUndo(state: UndoHistorySnapshot): {
  snapshot: UndoHistorySnapshot
  step: RevStep | null
} {
  if (state.cursor < 0) return { snapshot: state, step: null }
  const step = state.steps[state.cursor]
  return { snapshot: { steps: state.steps, cursor: state.cursor - 1 }, step }
}

/** Increment the cursor; return the step to re-apply (or null at the top). */
function applyRedo(state: UndoHistorySnapshot): {
  snapshot: UndoHistorySnapshot
  step: RevStep | null
} {
  if (state.cursor >= state.steps.length - 1) {
    return { snapshot: state, step: null }
  }
  const cursor = state.cursor + 1
  return { snapshot: { steps: state.steps, cursor }, step: state.steps[cursor] }
}

/** Drop the oldest steps once the cap is exceeded (can't undo past `MAX_HISTORY`). */
function capHistory(state: UndoHistorySnapshot): UndoHistorySnapshot {
  if (state.steps.length <= MAX_HISTORY) return state
  const overflow = state.steps.length - MAX_HISTORY
  return {
    steps: state.steps.slice(overflow),
    cursor: Math.max(state.cursor - overflow, -1),
  }
}

/** Generate a stable step id. `crypto.randomUUID` in browsers/bun; a fallback elsewhere. */
function generateStepId(): string {
  if (
    typeof crypto !== 'undefined' &&
    typeof crypto.randomUUID === 'function'
  ) {
    return crypto.randomUUID()
  }
  return `${Date.now()}-${Math.random().toString(36).slice(2)}`
}

/** Build a `RevStep` with a generated id. */
export function makeRevStep(
  messageId: string,
  sourceId: string,
  diff: MessageChangeDiff,
): RevStep {
  return {
    id: generateStepId(),
    messageId,
    sourceId,
    diff,
    createdAt: Date.now(),
  }
}

/**
 * Base store: in-memory cache + cursor logic + listeners, backed by a
 * persistence seam implemented by subclasses.
 */
abstract class BaseUndoHistoryStore implements UndoHistoryStore {
  protected state: UndoHistorySnapshot = { steps: [], cursor: -1 }
  private loaded = false
  private readonly listeners = new Set<(s: UndoHistorySnapshot) => void>()

  protected abstract loadPersisted(): Promise<UndoHistorySnapshot | null>
  protected abstract savePersisted(snapshot: UndoHistorySnapshot): Promise<void>
  protected abstract clearPersisted(): Promise<void>

  async load(): Promise<UndoHistorySnapshot> {
    if (!this.loaded) {
      const persisted = await this.loadPersisted()
      if (persisted) this.state = persisted
      this.loaded = true
    }
    return this.state
  }

  snapshot(): UndoHistorySnapshot {
    return this.state
  }

  async pushForward(step: RevStep): Promise<UndoHistorySnapshot> {
    await this.load()
    this.state = capHistory(applyPushForward(this.state, step))
    this.notify()
    await this.savePersisted(this.state)
    return this.state
  }

  async navigateUndo(): Promise<RevStep | null> {
    await this.load()
    const result = applyUndo(this.state)
    this.state = result.snapshot
    this.notify()
    await this.savePersisted(this.state)
    return result.step
  }

  async navigateRedo(): Promise<RevStep | null> {
    await this.load()
    const result = applyRedo(this.state)
    this.state = result.snapshot
    this.notify()
    await this.savePersisted(this.state)
    return result.step
  }

  async clear(): Promise<void> {
    this.state = { steps: [], cursor: -1 }
    this.loaded = true
    this.notify()
    await this.clearPersisted()
  }

  subscribe(listener: (s: UndoHistorySnapshot) => void): () => void {
    this.listeners.add(listener)
    return () => {
      this.listeners.delete(listener)
    }
  }

  private notify(): void {
    for (const listener of this.listeners) listener(this.state)
  }
}

/**
 * In-memory history store for tests + SSR/no-IndexedDB. An optional shared
 * `backing` object lets two store instances share persisted state — used to
 * test reload survival without a real IndexedDB.
 */
export class MemoryUndoHistoryStore extends BaseUndoHistoryStore {
  private readonly backing: { snapshot: UndoHistorySnapshot | null }

  constructor(
    backing: { snapshot: UndoHistorySnapshot | null } = { snapshot: null },
  ) {
    super()
    this.backing = backing
  }

  protected async loadPersisted(): Promise<UndoHistorySnapshot | null> {
    return this.backing.snapshot
  }

  protected async savePersisted(snapshot: UndoHistorySnapshot): Promise<void> {
    this.backing.snapshot = snapshot
  }

  protected async clearPersisted(): Promise<void> {
    this.backing.snapshot = null
  }
}

const DB_NAME = 'posthaste-replica'
const STORE_NAME = 'undoHistory'
const DB_VERSION = 2 // bumped from the outbox's v1 (new object store)
const RECORD_KEY = 'main'

function openConnection(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(DB_NAME, DB_VERSION)
    request.onupgradeneeded = () => {
      const connection = request.result
      if (!connection.objectStoreNames.contains(STORE_NAME)) {
        connection.createObjectStore(STORE_NAME, { keyPath: 'key' })
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
 * IndexedDB-backed history. The whole snapshot (steps + cursor) is one record
 * under a fixed key — the history is small (capped at `MAX_HISTORY`) so there is
 * no per-step indexing. Shares the `posthaste-replica` DB with the outbox.
 */
export class IndexedDbUndoHistoryStore extends BaseUndoHistoryStore {
  private connection: Promise<IDBDatabase> | undefined

  private db(): Promise<IDBDatabase> {
    this.connection ??= openConnection()
    return this.connection
  }

  protected async loadPersisted(): Promise<UndoHistorySnapshot | null> {
    const connection = await this.db()
    const transaction = connection.transaction(STORE_NAME, 'readonly')
    const record = await runRequest(
      transaction.objectStore(STORE_NAME).get(RECORD_KEY) as IDBRequest<
        { key: string; snapshot: UndoHistorySnapshot } | undefined
      >,
    )
    return record?.snapshot ?? null
  }

  protected async savePersisted(snapshot: UndoHistorySnapshot): Promise<void> {
    const connection = await this.db()
    await new Promise<void>((resolve, reject) => {
      const transaction = connection.transaction(STORE_NAME, 'readwrite')
      transaction.oncomplete = () => resolve()
      transaction.onerror = () => reject(transaction.error)
      transaction.onabort = () => reject(transaction.error)
      transaction.objectStore(STORE_NAME).put({ key: RECORD_KEY, snapshot })
    })
  }

  protected async clearPersisted(): Promise<void> {
    const connection = await this.db()
    await new Promise<void>((resolve, reject) => {
      const transaction = connection.transaction(STORE_NAME, 'readwrite')
      transaction.oncomplete = () => resolve()
      transaction.onerror = () => reject(transaction.error)
      transaction.onabort = () => reject(transaction.error)
      transaction.objectStore(STORE_NAME).delete(RECORD_KEY)
    })
  }
}

/**
 * The default history store: durable IndexedDB when available, otherwise an
 * in-memory fallback.
 */
export function defaultUndoHistoryStore(): UndoHistoryStore {
  if (typeof indexedDB !== 'undefined') {
    return new IndexedDbUndoHistoryStore()
  }
  return new MemoryUndoHistoryStore()
}

// --- singleton (mirrors `runtimeSessionClient`) ---

let singletonStore: UndoHistoryStore | undefined

/** The process-wide history store the adapter (forward actions) + hook (undo/redo) share. */
export function getUndoHistoryStore(): UndoHistoryStore {
  singletonStore ??= defaultUndoHistoryStore()
  return singletonStore
}

/** Replace the singleton store (tests inject an in-memory store). */
export function setUndoHistoryStoreForTesting(store: UndoHistoryStore): void {
  singletonStore = store
}

/** Reset the singleton (tests restore the default between cases). */
export function resetUndoHistoryStoreForTesting(): void {
  singletonStore = undefined
}
