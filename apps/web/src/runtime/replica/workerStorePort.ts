/**
 * `StorePort` over a Web Worker that hosts the client WASM entity store.
 *
 * This is the payoff of the async `StorePort` boundary: the CPU-bound store
 * (ingest + projection) runs on a worker thread, so a sync burst (e.g. a
 * post-repair full re-sync) can't freeze the UI thread. The renderer keeps
 * React, React Query, mutation translation, and the durable outbox/undo.
 *
 * The protocol is a request/response over `postMessage`: each call carries an
 * id, the method name, and its JSON-string args (the same payload that crosses
 * the WASM boundary today), and the worker replies with the method's result.
 * The controller already awaits + serializes store ops, so at most one call is
 * ever in flight — the worker processes them in FIFO arrival order.
 *
 * @spec docs/eph/DESIGN-L2-replica-worker-isolation
 */
import type { SettlementVerdict } from './handle'
import type { StorePort } from './storePort'

/** A method name on {@link StorePort}. */
export type StoreMethod = keyof StorePort

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
  terminate?(): void
}

export class WorkerStorePort implements StorePort {
  private readonly worker: ReplicaWorkerLike
  private nextId = 1
  private readonly pending = new Map<
    number,
    { resolve: (value: unknown) => void; reject: (error: unknown) => void }
  >()

  /** Resolves when the worker has loaded its WASM store; rejects if init fails.
   *  The host awaits this (with a timeout) to decide worker-vs-in-process. */
  readonly ready: Promise<void>
  private resolveReady!: () => void
  private rejectReady!: (error: unknown) => void

  constructor(worker: ReplicaWorkerLike) {
    this.worker = worker
    this.ready = new Promise<void>((resolve, reject) => {
      this.resolveReady = resolve
      this.rejectReady = reject
    })
    // A rejected `ready` that nobody is awaiting yet must not crash as an
    // unhandled rejection; the host attaches its own handler.
    this.ready.catch(() => {})
    this.worker.addEventListener('message', (event) =>
      this.onMessage(event.data),
    )
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
    this.pending.delete(response.id)
    if (response.ok) {
      entry.resolve(response.result)
    } else {
      entry.reject(new Error(response.error))
    }
  }

  private call<T>(method: StoreMethod, args: unknown[]): Promise<T> {
    const id = this.nextId++
    return new Promise<T>((resolve, reject) => {
      this.pending.set(id, {
        resolve: resolve as (value: unknown) => void,
        reject,
      })
      this.worker.postMessage({ id, method, args })
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
 */
export function createWorkerStorePort(): WorkerStorePort {
  const worker = new Worker(new URL('./storeWorker.ts', import.meta.url), {
    type: 'module',
  })
  return new WorkerStorePort(worker)
}
