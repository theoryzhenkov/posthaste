/**
 * Durable home of the client-owned undo/redo history — Phase 2 of the synced-
 * history refactor. Per-account partitions: each account has its own
 * `RevStep[]` + cursor (the singleton mixing of Phase 1 is gone). The global
 * Ctrl+Z merges per-account histories by `createdAt` (the latest undoable step
 * across all accounts) — no globally-ordered log needed. Navigation is LOCAL:
 * chained undo pops the cursor in memory + returns each step to invert, so N
 * undos do not cost N round trips. The diff is captured client-side
 * (`captureMutationDiffJson`); the runtime's per-session seq-keyed stacks are
 * retired. Persisted alongside the outbox (IndexedDB) so it survives reload.
 *
 * The store is the mirror of the server-authoritative `RevLog` synced view
 * (Phase 2 Slice 5b): `reconcileWithServer` adopts the server's steps + cursor
 * per account, with an optimism guard so a local move isn't reverted by a stale
 * server view. Steps carry stable `id`s (not session seqs) + cursor moves are
 * idempotent id-keyed assignments.
 *
 * @spec docs/eph/DESIGN-L2-undo-redo-revlog-contract
 */
import type { MessageChangeDiff } from './handle'
import { openReplicaDatabase, UNDO_HISTORY_STORE } from './replicaDatabase'

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
 * The server's `RevLogStep` wire shape (serde `camelCase`) — one row of the
 * per-account `rev_log`. The Phase 2 synced view serves a `RevLogSnapshotWire`
 * of these; the client mirror translates them to {@link RevStep}.
 *
 * @spec docs/eph/DESIGN-L2-undo-redo-revlog-contract
 */
export interface RevLogStepWire {
  /** Globally-orderable id (ULID); the cursor key. Maps to {@link RevStep.id}. */
  stepId: string
  /** Per-account monotonic append order (the sync delta cursor). */
  seq: number
  messageId: string
  sourceId: string
  /** `MessageChangeDiff` JSON (`{keywords, mailboxes}{added, removed}`). */
  diff: MessageChangeDiff
  /** ISO-8601 timestamp; for ordering/display only (`stepId`/`seq` order). */
  createdAt: string
}

/**
 * The server's `RevLogSnapshot` wire shape — the read result behind the `RevLog`
 * synced view. The mirror reconciles its local {@link UndoHistorySnapshot} with
 * this (translate + sort steps by `seq`; derive the cursor index from
 * `cursorStepId`).
 *
 * @spec docs/eph/DESIGN-L2-undo-redo-revlog-contract
 */
export interface RevLogSnapshotWire {
  steps: RevLogStepWire[]
  cursor: {
    /** The topmost APPLIED step (`null` = all undone, or empty account). */
    cursorStepId: string | null
    /** The undone step_ids above the cursor, in `seq` order. */
    redoTail: string[]
  }
}

/**
 * One account's history state: `steps[0..cursor]` are applied (undoable, newest
 * first), `steps[cursor+1..]` are undone (redoable, oldest first). `cursor` is
 * -1 when everything has been undone.
 */
export interface UndoHistorySnapshot {
  steps: RevStep[]
  cursor: number
}

/** The result of a global undo/redo: the step to apply + which account it's in. */
export interface UndoRedoResult {
  step: RevStep
  accountId: string
}

/**
 * The store seam: the adapter (per-account forward actions), the mirror
 * (per-account `RevLog` view reconciliation), + the hook (global undo/redo
 * merge) all drive this. Per-account partitions; the global undo/redo merge
 * across accounts by `createdAt`.
 */
export interface UndoHistoryStore {
  /** Load all persisted per-account snapshots (once). */
  load(): Promise<void>
  /** The cached snapshot for an account (empty if unseen). Sync read for the hook + `sendRevCursor`. */
  snapshot(accountId: string): UndoHistorySnapshot
  /** Record a forward action on an account: truncate the redoable tail, append, advance. */
  pushForward(accountId: string, step: RevStep): Promise<void>
  /** Reconcile an account's partition with the server-authoritative `RevLog` view. */
  reconcileWithServer(
    accountId: string,
    server: RevLogSnapshotWire,
  ): Promise<void>
  /** Empty an account's history. */
  clear(accountId: string): Promise<void>
  /** Global: is there any undoable step across all accounts? */
  canUndo(): boolean
  /** Global: is there any redoable step across all accounts? */
  canRedo(): boolean
  /**
   * Global undo: find the account whose topmost-applied step has the latest
   * `createdAt`, move its cursor down, return the step to invert + the account.
   */
  undo(): Promise<UndoRedoResult | null>
  /**
   * Global redo: find the account whose next-redoable step has the latest
   * `createdAt`, move its cursor up, return the step to re-apply + the account.
   */
  redo(): Promise<UndoRedoResult | null>
  /** Notify on any history change (the hook re-reads canUndo/canRedo). */
  subscribe(listener: () => void): () => void
}

/// Upper bound on retained history per account (matches the runtime's former `MAX_HISTORY`).
const MAX_HISTORY = 50

/// Phase 2 optimism guard config. The timeout is how long a local cursor move
/// stays optimistic before the mirror converges to the server's (possibly
/// stale) cursor — a safety valve for a lost/overridden `revCursor` so the
/// client re-converges instead of drifting forever. The `revCursor` round-trip
/// is fast (<1s local); 5s is generous. Mutable for tests (the timeout path).
/// @spec docs/eph/DESIGN-L2-undo-redo-revlog-contract
const optimismConfig = { timeoutMs: 5000 }

/** Translate a server {@link RevLogStepWire} to a client {@link RevStep}. */
function translateStep(step: RevLogStepWire): RevStep {
  return {
    id: step.stepId,
    messageId: step.messageId,
    sourceId: step.sourceId,
    diff: step.diff,
    createdAt: Date.parse(step.createdAt) || 0,
  }
}

/** Translate + sort the server's steps by `seq` (the per-account append order). */
function translateAndSort(steps: RevLogStepWire[]): RevStep[] {
  return steps
    .slice()
    .sort((a, b) => a.seq - b.seq)
    .map(translateStep)
}

/** The cursor index for a `cursorStepId` (`null` = -1, all undone). */
function indexForStepId(steps: RevStep[], stepId: string | null): number {
  if (stepId === null) return -1
  return steps.findIndex((s) => s.id === stepId)
}

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

/** The topmost APPLIED step of an account (the undo target), or null. */
function topApplied(snap: UndoHistorySnapshot): RevStep | null {
  return snap.cursor >= 0 ? (snap.steps[snap.cursor] ?? null) : null
}

/** The next redoable step of an account (the redo target), or null. */
function nextRedoable(snap: UndoHistorySnapshot): RevStep | null {
  return snap.cursor < snap.steps.length - 1
    ? (snap.steps[snap.cursor + 1] ?? null)
    : null
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

const EMPTY_SNAPSHOT: UndoHistorySnapshot = { steps: [], cursor: -1 }

/**
 * Base multi-account store: a `Map<accountId, UndoHistorySnapshot>` cache +
 * per-account cursor logic + global undo/redo merge, backed by a persistence
 * seam (per-account records) implemented by subclasses.
 */
abstract class BaseMultiAccountUndoHistoryStore implements UndoHistoryStore {
  protected state = new Map<string, UndoHistorySnapshot>()
  private loaded = false
  private readonly listeners = new Set<() => void>()
  /**
   * Phase 2 optimism guard: per-account pending cursor (the optimistic
   * `cursorStepId` + when), awaiting server confirmation via the `RevLog` view.
   * While set, {@link reconcileWithServer} skips adoption of a stale server
   * cursor for that account (the server hasn't processed the `revCursor`/append
   * yet) so the optimistic move isn't reverted. Cleared on confirm (server
   * echoes the same cursor) or after {@link optimismConfig.timeoutMs}
   * (lost/overridden → converge). Transient — not persisted.
   */
  private readonly pending = new Map<
    string,
    { stepId: string | null; sentAt: number }
  >()

  protected abstract loadAllPersisted(): Promise<
    Map<string, UndoHistorySnapshot>
  >
  protected abstract savePersisted(
    accountId: string,
    snapshot: UndoHistorySnapshot,
  ): Promise<void>
  protected abstract clearPersisted(accountId: string): Promise<void>

  /** Mark an account's post-move cursor as optimistically pending confirmation. */
  private markPending(accountId: string, snapshot: UndoHistorySnapshot): void {
    this.pending.set(accountId, {
      stepId:
        snapshot.cursor >= 0
          ? (snapshot.steps[snapshot.cursor]?.id ?? null)
          : null,
      sentAt: Date.now(),
    })
  }

  async load(): Promise<void> {
    if (!this.loaded) {
      this.state = await this.loadAllPersisted()
      this.loaded = true
    }
  }

  snapshot(accountId: string): UndoHistorySnapshot {
    return this.state.get(accountId) ?? EMPTY_SNAPSHOT
  }

  async pushForward(accountId: string, step: RevStep): Promise<void> {
    await this.load()
    const current = this.state.get(accountId) ?? EMPTY_SNAPSHOT
    const updated = capHistory(applyPushForward(current, step))
    this.state.set(accountId, updated)
    this.markPending(accountId, updated)
    this.notify()
    await this.savePersisted(accountId, updated)
  }

  async clear(accountId: string): Promise<void> {
    await this.load()
    this.state.delete(accountId)
    this.pending.delete(accountId)
    this.notify()
    await this.clearPersisted(accountId)
  }

  canUndo(): boolean {
    for (const snap of this.state.values()) {
      if (topApplied(snap)) return true
    }
    return false
  }

  canRedo(): boolean {
    for (const snap of this.state.values()) {
      if (nextRedoable(snap)) return true
    }
    return false
  }

  async undo(): Promise<UndoRedoResult | null> {
    await this.load()
    // Find the account whose topmost-applied step has the latest `createdAt`.
    let target: { accountId: string; step: RevStep } | null = null
    for (const [accountId, snap] of this.state) {
      const top = topApplied(snap)
      if (top && (!target || top.createdAt > target.step.createdAt)) {
        target = { accountId, step: top }
      }
    }
    if (!target) return null
    const result = applyUndo(this.state.get(target.accountId)!)
    this.state.set(target.accountId, result.snapshot)
    this.markPending(target.accountId, result.snapshot)
    this.notify()
    await this.savePersisted(target.accountId, result.snapshot)
    return { step: result.step!, accountId: target.accountId }
  }

  async redo(): Promise<UndoRedoResult | null> {
    await this.load()
    // Find the account whose next-redoable step has the latest `createdAt`.
    let target: { accountId: string; step: RevStep } | null = null
    for (const [accountId, snap] of this.state) {
      const next = nextRedoable(snap)
      if (next && (!target || next.createdAt > target.step.createdAt)) {
        target = { accountId, step: next }
      }
    }
    if (!target) return null
    const result = applyRedo(this.state.get(target.accountId)!)
    this.state.set(target.accountId, result.snapshot)
    this.markPending(target.accountId, result.snapshot)
    this.notify()
    await this.savePersisted(target.accountId, result.snapshot)
    return { step: result.step!, accountId: target.accountId }
  }

  /**
   * Phase 2: reconcile an account's partition with the server-authoritative
   * `RevLog` view. Adopts the server's steps + cursor unless a local move is
   * in-flight for that account (`pending`):
   *
   * - No pending move → adopt (cross-device convergence: this device sees
   *   other devices' forward actions + cursor for the account).
   * - Pending + the server echoes the optimistic cursor → confirmed → adopt +
   *   clear pending (the `revCursor`/append was processed).
   * - Pending + timed out (`optimismConfig.timeoutMs`) → lost/overridden →
   *   converge to the server's cursor + clear pending (last-writer-wins).
   * - Pending + otherwise → stale → skip adoption (keep the optimistic local
   *   state; preserves in-flight forward-action steps the server hasn't
   *   confirmed yet).
   *
   * @spec docs/eph/DESIGN-L2-undo-redo-revlog-contract
   */
  async reconcileWithServer(
    accountId: string,
    server: RevLogSnapshotWire,
  ): Promise<void> {
    await this.load()
    const serverSteps = translateAndSort(server.steps)
    const serverCursorStepId = server.cursor.cursorStepId
    const pending = this.pending.get(accountId)
    if (pending) {
      if (serverCursorStepId === pending.stepId) {
        // Confirmed: the server processed our move. Fall through to adopt.
        this.pending.delete(accountId)
      } else if (Date.now() - pending.sentAt >= optimismConfig.timeoutMs) {
        // Lost/overridden: converge to the server's cursor. Fall through.
        this.pending.delete(accountId)
      } else {
        // Stale: the server hasn't caught up. Keep the optimistic local state.
        return
      }
    }
    const updated = {
      steps: serverSteps,
      cursor: indexForStepId(serverSteps, serverCursorStepId),
    }
    this.state.set(accountId, updated)
    this.notify()
    await this.savePersisted(accountId, updated)
  }

  subscribe(listener: () => void): () => void {
    this.listeners.add(listener)
    return () => {
      this.listeners.delete(listener)
    }
  }

  private notify(): void {
    for (const listener of this.listeners) listener()
  }
}

/**
 * In-memory history store for tests + SSR/no-IndexedDB. An optional shared
 * `backing` map lets two store instances share persisted state — used to test
 * reload survival without a real IndexedDB.
 */
export class MemoryUndoHistoryStore extends BaseMultiAccountUndoHistoryStore {
  private readonly backing: Map<string, UndoHistorySnapshot>

  constructor(backing: Map<string, UndoHistorySnapshot> = new Map()) {
    super()
    this.backing = backing
  }

  protected async loadAllPersisted(): Promise<
    Map<string, UndoHistorySnapshot>
  > {
    return new Map(this.backing)
  }

  protected async savePersisted(
    accountId: string,
    snapshot: UndoHistorySnapshot,
  ): Promise<void> {
    this.backing.set(accountId, snapshot)
  }

  protected async clearPersisted(accountId: string): Promise<void> {
    this.backing.delete(accountId)
  }
}

const STORE_NAME = UNDO_HISTORY_STORE
/// The legacy Phase 1 single-account record key. Filtered on load (the Phase 2
/// mirror re-syncs from the server's `RevLog` view, so dropping it loses no
/// synced state).
const LEGACY_RECORD_KEY = 'main'

function runRequest<T>(request: IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    request.onsuccess = () => resolve(request.result)
    request.onerror = () => reject(request.error)
  })
}

/**
 * IndexedDB-backed history. Each account's snapshot (steps + cursor) is one
 * record keyed by the accountId — the history is small (capped at
 * `MAX_HISTORY` per account) so there is no per-step indexing. Shares the
 * `posthaste-replica` DB with the outbox. The legacy Phase 1 `'main'` record is
 * dropped on load (the mirror re-syncs from the server).
 */
export class IndexedDbUndoHistoryStore extends BaseMultiAccountUndoHistoryStore {
  private connection: Promise<IDBDatabase> | undefined

  private db(): Promise<IDBDatabase> {
    this.connection ??= openReplicaDatabase()
    return this.connection
  }

  protected async loadAllPersisted(): Promise<
    Map<string, UndoHistorySnapshot>
  > {
    const connection = await this.db()
    const transaction = connection.transaction(STORE_NAME, 'readonly')
    const records = await runRequest(
      transaction.objectStore(STORE_NAME).getAll() as IDBRequest<
        { key: string; snapshot: UndoHistorySnapshot }[]
      >,
    )
    const map = new Map<string, UndoHistorySnapshot>()
    for (const record of records) {
      // Drop the legacy Phase 1 single-account record (the mirror re-syncs).
      if (record.key === LEGACY_RECORD_KEY) continue
      map.set(record.key, record.snapshot)
    }
    return map
  }

  protected async savePersisted(
    accountId: string,
    snapshot: UndoHistorySnapshot,
  ): Promise<void> {
    const connection = await this.db()
    await new Promise<void>((resolve, reject) => {
      const transaction = connection.transaction(STORE_NAME, 'readwrite')
      transaction.oncomplete = () => resolve()
      transaction.onerror = () => reject(transaction.error)
      transaction.onabort = () => reject(transaction.error)
      transaction.objectStore(STORE_NAME).put({ key: accountId, snapshot })
    })
  }

  protected async clearPersisted(accountId: string): Promise<void> {
    const connection = await this.db()
    await new Promise<void>((resolve, reject) => {
      const transaction = connection.transaction(STORE_NAME, 'readwrite')
      transaction.oncomplete = () => resolve()
      transaction.onerror = () => reject(transaction.error)
      transaction.onabort = () => reject(transaction.error)
      transaction.objectStore(STORE_NAME).delete(accountId)
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

/**
 * @internal Phase 2 optimism-guard timeout (mutable for tests — the timeout
 * re-convergence path). Restore to 5000 after mutating.
 */
export const _optimismConfigForTesting = optimismConfig
