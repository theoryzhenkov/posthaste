/**
 * `StorePort` over a Web Worker that hosts the client WASM entity store.
 *
 * This is the payoff of the async `StorePort` boundary: the CPU-bound store
 * (ingest + projection) runs on a worker thread, so a sync burst (e.g. a
 * post-repair full re-sync) can't freeze the UI thread. The renderer keeps
 * React, React Query, mutation translation, and the durable pending set/undo.
 *
 * The protocol is a request/response over `postMessage`: each call carries an
 * id, the method name, and its JSON-string args (the same payload that crosses
 * the WASM boundary today), and the worker replies with the method's result.
 * The controller already awaits + serializes store ops, so at most one call is
 * ever in flight — the worker processes them in FIFO arrival order.
 *
 * @spec docs/eph/DESIGN-L2-replica-worker-isolation
 */
import { LOG_EVENTS, syncLogger } from '../../logger'

import type { SettlementVerdict } from './handle'
import type { StorePort } from './storePort'

/** A method name on {@link StorePort} that is dispatched across the worker
 *  boundary. `setReseedHook` is port-local (it wires the controller's re-seed
 *  callback) and is never sent as a worker request, so it is excluded. */
export type StoreMethod = Exclude<keyof StorePort, 'setReseedHook'>

export interface StoreWorkerRequest {
  id: number
  method: StoreMethod
  args: unknown[]
}

export type StoreWorkerResponse =
  | { id: number; ok: true; result: unknown }
  | { id: number; ok: false; error: string }

/** Worker → main: the one-time readiness handshake. The worker posts this once
 *  the WASM store has loaded (`ok: true`) or if init failed (`ok: false`). It
 *  lets the host probe worker viability before relying on it, and fall back to
 *  the in-process store if a webview can't run the worker. */
export type StoreWorkerReady =
  | { type: 'ready'; ok: true }
  | { type: 'ready'; ok: false; error: string }

export type StoreWorkerOutbound = StoreWorkerResponse | StoreWorkerReady

/** The minimal worker surface used here — satisfied by a real `Worker` and by
 *  the loopback fake the unit tests drive. */
export interface ReplicaWorkerLike {
  postMessage(message: StoreWorkerRequest): void
  addEventListener(
    type: 'message',
    listener: (event: { data: StoreWorkerOutbound }) => void,
  ): void
  addEventListener(
    type: 'error',
    listener: (event: { message?: string }) => void,
  ): void
  terminate?(): void
}

/**
 * How long a single `call()` round trip may take before the worker is
 * considered wedged (W2 / N20). The store's ops are CPU-bound JSON-in/JSON-out
 * (see the module doc) — no network/IO waits inside them — so a real reply
 * arrives in low milliseconds; this is a liveness deadline, not a perf budget.
 */
export const WORKER_CALL_TIMEOUT_MS = 10_000

/**
 * How many times a wedged worker is terminated + respawned + the timed-out
 * call replayed before giving up and failing the port outright (bounded, so a
 * worker that is wedged for a structural reason — e.g. its module asset can't
 * load — doesn't respawn forever).
 */
export const WORKER_CALL_MAX_RESTARTS = 2

interface PendingCall {
  method: StoreMethod
  args: unknown[]
  resolve: (value: unknown) => void
  reject: (error: unknown) => void
  timer: ReturnType<typeof setTimeout>
  attempt: number
}

export interface WorkerStorePortOptions {
  /** Spawn a fresh worker to replace one that timed out. Omitted (e.g. the
   *  unit tests that don't exercise the watchdog) disables restart-on-timeout:
   *  a timeout then just fails the port, same as a reported `error` event. */
  spawnWorker?: () => ReplicaWorkerLike
  /** Override the per-call round-trip deadline (tests). */
  callTimeoutMs?: number
  /** Override the bounded restart-attempt budget (tests). */
  maxRestarts?: number
}

export class WorkerStorePort implements StorePort {
  private worker: ReplicaWorkerLike
  private readonly spawnWorker: (() => ReplicaWorkerLike) | undefined
  private readonly callTimeoutMs: number
  private readonly maxRestarts: number
  private nextId = 1
  private readonly pending = new Map<number, PendingCall>()

  /** Resolves when the worker has loaded its WASM store; rejects if init fails.
   *  The host awaits this (with a timeout) to decide worker-vs-in-process. */
  readonly ready: Promise<void>
  private resolveReady!: () => void
  private rejectReady!: (error: unknown) => void
  /** Set once the worker is unusable (crashed, terminated, or exhausted its
   *  restart budget). Further calls reject immediately instead of hanging the
   *  controller's store queue. */
  private dead = false
  /** The controller's re-seed callback (CL-C1), invoked on a respawn to rebuild
   *  the fresh worker's store before the timed-out call is replayed. */
  private reseed: (() => Promise<void>) | undefined
  /** True while a respawn re-seed is driving its own calls on the fresh worker.
   *  A timeout during this window means the replacement is ALSO wedged — fail
   *  the port cleanly instead of respawn-looping (bounded, no silent emptiness). */
  private reseeding = false

  constructor(worker: ReplicaWorkerLike, options: WorkerStorePortOptions = {}) {
    this.worker = worker
    this.spawnWorker = options.spawnWorker
    this.callTimeoutMs = options.callTimeoutMs ?? WORKER_CALL_TIMEOUT_MS
    this.maxRestarts = options.maxRestarts ?? WORKER_CALL_MAX_RESTARTS
    this.ready = new Promise<void>((resolve, reject) => {
      this.resolveReady = resolve
      this.rejectReady = reject
    })
    // A rejected `ready` that nobody is awaiting yet must not crash as an
    // unhandled rejection; the host attaches its own handler.
    this.ready.catch(() => {})
    this.attach(this.worker)
  }

  /** Wire message/error listeners onto (a possibly just-respawned) worker. */
  private attach(worker: ReplicaWorkerLike): void {
    worker.addEventListener('message', (event) => this.onMessage(event.data))
    // A worker that dies AFTER init would otherwise leave every in-flight call
    // unsettled — and since the controller chains store ops, that hangs the
    // whole store. Fail fast instead: reject pending + future calls so the
    // failure surfaces as an error rather than a silent freeze.
    worker.addEventListener('error', (event) =>
      this.fail(
        new Error(`replica worker error: ${event.message ?? 'unknown'}`),
      ),
    )
  }

  /** Tear the worker down + reject everything outstanding. Idempotent. */
  private fail(error: Error): void {
    if (this.dead) {
      return
    }
    this.dead = true
    this.rejectReady(error)
    for (const entry of this.pending.values()) {
      clearTimeout(entry.timer)
      entry.reject(error)
    }
    this.pending.clear()
  }

  /** CL-C1: register the controller's re-seed callback. The port owns WHEN it
   *  runs (immediately after a respawn, before the replay); the controller owns
   *  HOW (re-register views + re-fold the pending set from the state it holds on
   *  the main thread). */
  setReseedHook(reseed: () => Promise<void>): void {
    this.reseed = reseed
  }

  /** Release the worker (e.g. the host probed it unusable and fell back). */
  terminate(): void {
    this.fail(new Error('replica worker terminated'))
    this.worker.terminate?.()
  }

  private onMessage(message: StoreWorkerOutbound): void {
    if ('id' in message) {
      this.onResponse(message)
      return
    }
    // The readiness handshake (no `id`).
    if (message.ok) {
      this.resolveReady()
    } else {
      this.rejectReady(new Error(message.error))
    }
  }

  private onResponse(response: StoreWorkerResponse): void {
    const entry = this.pending.get(response.id)
    if (!entry) {
      return
    }
    clearTimeout(entry.timer)
    this.pending.delete(response.id)
    if (response.ok) {
      entry.resolve(response.result)
    } else {
      entry.reject(new Error(response.error))
    }
  }

  /**
   * A `call()`'s round trip missed {@link WorkerStorePort.callTimeoutMs}: the
   * worker is presumed wedged (panicked into an unresponsive state without
   * firing the `error` event — e.g. an infinite loop in the WASM store).
   * Terminate it, spawn a replacement, RE-SEED its store (CL-C1), and replay the
   * SAME request on the fresh worker (bounded by `maxRestarts`) so the caller
   * sees a slow-but-eventually-answered call against a store that still holds
   * its views/bases/folds — instead of an unhandled hang, a spurious failure, or
   * (the CL-C1 bug) a replay that "succeeds" against a BRAND-NEW empty store and
   * reports row-dropping emptiness as authoritative. Exhausting the budget fails
   * the port outright — a worker wedged this persistently is not going to recover.
   */
  private onTimeout(id: number): void {
    const entry = this.pending.get(id)
    if (!entry) {
      return
    }
    this.pending.delete(id)
    // A timeout WHILE re-seeding means the just-respawned worker is also wedged.
    // Don't respawn-loop into it: fail the port so the failure surfaces cleanly
    // rather than churning replacements (bounded — CL-C1's "clean surfaced
    // failure, never silent emptiness" arm).
    if (
      !this.spawnWorker ||
      this.reseeding ||
      entry.attempt >= this.maxRestarts
    ) {
      const error = new Error(
        `replica worker call timed out (method=${entry.method}, attempt=${entry.attempt + 1})`,
      )
      entry.reject(error)
      // The rest of the pending queue (in practice at most this one call —
      // the controller serializes store ops) can't be trusted behind a worker
      // this unresponsive either.
      this.fail(error)
      return
    }
    this.worker.terminate?.()
    const fresh = this.spawnWorker()
    this.worker = fresh
    this.attach(fresh)
    syncLogger.warn(
      {
        event: LOG_EVENTS.replicaWorkerRestarted,
        method: entry.method,
        attempt: entry.attempt + 1,
        maxRestarts: this.maxRestarts,
      },
      'replica worker call timed out; respawned the worker and re-seeding its store before replaying the request',
    )
    void this.reseedAndReplay(id, entry)
  }

  /**
   * CL-C1: after a respawn, rebuild the fresh (empty) worker's store via the
   * controller's re-seed hook, THEN replay the timed-out request. The store
   * queue is parked on the timed-out call's promise (the controller serializes
   * ops and this call has not resolved), so the re-seed's own calls run on the
   * fresh worker with nothing else interleaving. A re-seed failure surfaces as a
   * clean port failure rather than a replay into a half-seeded store.
   */
  private async reseedAndReplay(id: number, entry: PendingCall): Promise<void> {
    if (this.reseed) {
      this.reseeding = true
      try {
        await this.reseed()
      } catch (error) {
        const err = error instanceof Error ? error : new Error(String(error))
        entry.reject(err)
        this.fail(err)
        return
      } finally {
        this.reseeding = false
      }
      // A re-seed call may have failed the port (e.g. the fresh worker also
      // wedged or crashed mid-seed); `entry` is already rejected by `fail`.
      if (this.dead) {
        return
      }
    }
    this.send(
      id,
      entry.method,
      entry.args,
      entry.resolve,
      entry.reject,
      entry.attempt + 1,
    )
  }

  private send(
    id: number,
    method: StoreMethod,
    args: unknown[],
    resolve: (value: unknown) => void,
    reject: (error: unknown) => void,
    attempt: number,
  ): void {
    const timer = setTimeout(() => this.onTimeout(id), this.callTimeoutMs)
    this.pending.set(id, { method, args, resolve, reject, timer, attempt })
    this.worker.postMessage({ id, method, args })
  }

  private call<T>(method: StoreMethod, args: unknown[]): Promise<T> {
    if (this.dead) {
      return Promise.reject(new Error('replica worker is no longer available'))
    }
    const id = this.nextId++
    return new Promise<T>((resolve, reject) => {
      this.send(
        id,
        method,
        args,
        resolve as (value: unknown) => void,
        reject,
        0,
      )
    })
  }

  registerViewJson(viewId: string, argsJson: string): Promise<void> {
    return this.call('registerViewJson', [viewId, argsJson])
  }
  setViewRowsJson(
    viewId: string,
    rowsJson: string,
    watermarkJson: string,
  ): Promise<void> {
    return this.call('setViewRowsJson', [viewId, rowsJson, watermarkJson])
  }
  closeView(viewId: string): Promise<void> {
    return this.call('closeView', [viewId])
  }
  ingestBatchJson(batchJson: string): Promise<void> {
    return this.call('ingestBatchJson', [batchJson])
  }
  acceptMutationJson(acceptJson: string): Promise<void> {
    return this.call('acceptMutationJson', [acceptJson])
  }
  settle(mutationId: string, outcome: SettlementVerdict): Promise<boolean> {
    return this.call('settle', [mutationId, outcome])
  }
  captureMutationDiffJson(
    messageId: string,
    assertionJson: string,
  ): Promise<string> {
    return this.call('captureMutationDiffJson', [messageId, assertionJson])
  }
  mailboxJson(mailboxId: string): Promise<string> {
    return this.call('mailboxJson', [mailboxId])
  }
  projectViewJson(viewId: string): Promise<string> {
    return this.call('projectViewJson', [viewId])
  }
  drainDirtyJson(): Promise<string> {
    return this.call('drainDirtyJson', [])
  }
  drainRetiredJson(): Promise<string> {
    return this.call('drainRetiredJson', [])
  }
}

/**
 * Create a `WorkerStorePort` backed by the bundled replica worker. Uses Vite's
 * `new Worker(new URL(...), { type: 'module' })` form so the worker (and its
 * WASM) are emitted as a separate chunk. Only called behind the worker flag;
 * never imported by the unit tests (which drive {@link WorkerStorePort} with a
 * loopback fake).
 *
 * Passes itself as `spawnWorker` so the port's timeout watchdog (W2) can
 * terminate + respawn a wedged worker instead of just failing outright.
 */
function spawnReplicaWorker(): Worker {
  return new Worker(new URL('./storeWorker.ts', import.meta.url), {
    type: 'module',
  })
}

export function createWorkerStorePort(): WorkerStorePort {
  return new WorkerStorePort(spawnReplicaWorker(), {
    spawnWorker: spawnReplicaWorker,
  })
}
