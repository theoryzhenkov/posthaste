/**
 * Replica store worker entry: hosts the WASM `EntityStoreHandle` off the UI
 * thread and serves `StorePort` method calls over `postMessage`.
 *
 * The WASM handle's methods are synchronous (JSON string in/out), so a request
 * runs without yielding; the only async point is the one-time WASM init. The
 * controller sends at most one call at a time, and pre-init calls resolve in
 * FIFO arrival order, so store ordering is preserved.
 *
 * @spec docs/eph/DESIGN-L2-replica-worker-isolation
 */
import { loadEntityStoreHandleFactory, type EntityStoreHandle } from './handle'
import type { StoreWorkerRequest, StoreWorkerResponse } from './workerStorePort'

// Cast the worker global so this file is independent of the DOM/WebWorker lib
// split — it only needs `postMessage` + `addEventListener('message')`.
const ctx = globalThis as unknown as {
  postMessage(message: StoreWorkerResponse): void
  addEventListener(
    type: 'message',
    listener: (event: { data: StoreWorkerRequest }) => void,
  ): void
}

let handle: EntityStoreHandle | null = null
const ready: Promise<void> = (async () => {
  const factory = await loadEntityStoreHandleFactory()
  handle = factory()
})()

function run(request: StoreWorkerRequest): StoreWorkerResponse {
  try {
    const method = handle![request.method] as (...args: unknown[]) => unknown
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

ctx.addEventListener('message', (event) => {
  const request = event.data
  if (handle) {
    ctx.postMessage(run(request))
  } else {
    // Before init completes: queue behind `ready`. `.then` callbacks run in the
    // order they were registered (= message arrival order), preserving FIFO.
    void ready.then(() => ctx.postMessage(run(request)))
  }
})
