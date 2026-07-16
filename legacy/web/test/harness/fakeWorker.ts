/**
 * The harness FAKE WORKER: a controllable `workerStorePort` stand-in.
 *
 * This UNIFIES the two ad-hoc loopback fakes the suite grew — the `LoopbackWorker`
 * in `workerStorePort.test.ts` and the loopback pieces the M31 watchdog test
 * drives — into one `LoopbackReplicaWorker` plus a kit that wires it into the
 * REAL `WorkerStorePort` with its watchdog options. The port is the shipped
 * one; only the worker across the `postMessage` boundary is faked, so the kit
 * proves the real restart/replay/dead-latch behavior (the M31 / F3 / F4 modes).
 *
 * Drive handles: `wedge()` (stop answering — the watchdog's target), `die()`
 * (fire the `error` event — the crash latch), `emitReady()` (the readiness
 * handshake), plus `spawnCount` / `terminatedCount` probes.
 *
 * @spec docs/eph/RFC-L2-client-resilience.md#D119
 */
import { loadRealHandleFactory } from './wasmHandle'
import type { EntityStoreHandle } from '../../src/runtime/replica/handle'
import {
  WorkerStorePort,
  type ReplicaWorkerLike,
  type StoreWorkerOutbound,
  type StoreWorkerRequest,
  type StoreWorkerResponse,
} from '../../src/runtime/replica/workerStorePort'

/** A responder maps a request to its response (or a pending promise). */
export type WorkerResponder = (
  request: StoreWorkerRequest,
) => StoreWorkerResponse | Promise<StoreWorkerResponse>

/**
 * A loopback "worker": `respond` produces each reply, delivered asynchronously
 * like a real `Worker` (one microtask hop). Controllable: `wedge()` makes it
 * stop answering (the watchdog fires), `die()` fires the `error` event.
 */
export class LoopbackReplicaWorker implements ReplicaWorkerLike {
  private messageListener:
    | ((event: { data: StoreWorkerOutbound }) => void)
    | null = null
  private errorListener: ((event: { message?: string }) => void) | null = null
  readonly received: StoreWorkerRequest[] = []
  terminated = false
  private wedged = false

  constructor(private readonly respond: WorkerResponder) {}

  addEventListener(
    type: 'message' | 'error',
    listener:
      | ((event: { data: StoreWorkerOutbound }) => void)
      | ((event: { message?: string }) => void),
  ): void {
    if (type === 'error') {
      this.errorListener = listener as (event: { message?: string }) => void
    } else {
      this.messageListener = listener as (event: {
        data: StoreWorkerOutbound
      }) => void
    }
  }

  postMessage(request: StoreWorkerRequest): void {
    this.received.push(request)
    if (this.wedged) {
      return // never answers — the port's watchdog must fire
    }
    void Promise.resolve(this.respond(request)).then((response) => {
      if (!this.wedged && !this.terminated) {
        this.messageListener?.({ data: response })
      }
    })
  }

  terminate(): void {
    this.terminated = true
  }

  /** Stop answering (and drop any in-flight reply): simulate a wedged worker. */
  wedge(): void {
    this.wedged = true
  }

  /** Emit the readiness handshake (`ready`). */
  emitReady(ok = true, error = 'wasm init failed'): void {
    this.messageListener?.({
      data: ok
        ? { type: 'ready', ok: true }
        : { type: 'ready', ok: false, error },
    })
  }

  /** Fire the worker `error` event (the thread died after init). */
  die(message = 'worker crashed'): void {
    this.errorListener?.({ message })
  }
}

/** A responder that runs the REAL wasm `EntityStoreHandle` (the store worker's
 *  own dispatch, mirrored) so loopback calls actually mutate/project. */
export function realHandleResponder(
  handle: EntityStoreHandle,
): WorkerResponder {
  return (request) => {
    try {
      const method = handle[request.method] as (...args: unknown[]) => unknown
      const result = method.apply(handle, request.args)
      return { id: request.id, ok: true, result }
    } catch (error) {
      return {
        id: request.id,
        ok: false,
        error: error instanceof Error ? error.message : String(error),
      }
    }
  }
}

export interface WorkerKit {
  /** The real `WorkerStorePort` under test, backed by the loopback worker. */
  port: WorkerStorePort
  /** The currently-live loopback worker (swapped on a watchdog restart). */
  current(): LoopbackReplicaWorker
  /** Wedge the live worker so its next call times out (drives the watchdog). */
  wedge(): void
  /** Fire the live worker's `error` event (the dead-worker latch, F4). */
  die(message?: string): void
  /** How many workers have been spawned (initial + watchdog restarts). */
  spawnCount(): number
  /** How many workers have been terminated. */
  terminatedCount(): number
}

export interface WorkerKitOptions {
  /** Build the responder for the Nth spawned worker (1-based). Defaults to a
   *  fresh real-handle responder each spawn. */
  responderForSpawn?: (
    spawn: number,
    handle: EntityStoreHandle,
  ) => WorkerResponder
  /** Per-call watchdog deadline (small in tests). */
  callTimeoutMs?: number
  /** Bounded restart budget. */
  maxRestarts?: number
}

/**
 * Build a real `WorkerStorePort` over loopback workers. Each spawn (initial +
 * every watchdog restart) mints a fresh worker; by default each runs a fresh
 * real wasm handle. Pass `responderForSpawn` to script the classic M31 shape
 * (first worker wedged, the replacement answers).
 */
export async function createWorkerKit(
  options: WorkerKitOptions = {},
): Promise<WorkerKit> {
  const makeHandle = await loadRealHandleFactory()
  let spawnCount = 0
  const workers: LoopbackReplicaWorker[] = []

  const spawnWorker = (): LoopbackReplicaWorker => {
    spawnCount += 1
    const handle = makeHandle()
    const responder =
      options.responderForSpawn?.(spawnCount, handle) ??
      realHandleResponder(handle)
    const worker = new LoopbackReplicaWorker(responder)
    workers.push(worker)
    return worker
  }

  const first = spawnWorker()
  const port = new WorkerStorePort(first, {
    spawnWorker,
    ...(options.callTimeoutMs !== undefined
      ? { callTimeoutMs: options.callTimeoutMs }
      : {}),
    ...(options.maxRestarts !== undefined
      ? { maxRestarts: options.maxRestarts }
      : {}),
  })
  // The port doesn't await `ready` for calls, but signal it so nothing hangs
  // if a caller (e.g. the store resolver) probes readiness.
  first.emitReady(true)

  return {
    port,
    current: () => workers[workers.length - 1]!,
    wedge: () => workers[workers.length - 1]!.wedge(),
    die: (message?: string) => workers[workers.length - 1]!.die(message),
    spawnCount: () => spawnCount,
    terminatedCount: () => workers.filter((w) => w.terminated).length,
  }
}
