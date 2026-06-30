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
import type {
  StoreWorkerOutbound,
  StoreWorkerRequest,
  StoreWorkerResponse,
} from './workerStorePort'

// Cast the worker global so this file is independent of the DOM/WebWorker lib
// split — it only needs `postMessage` + `addEventListener('message')`.
const ctx = globalThis as unknown as {
  postMessage(message: StoreWorkerOutbound): void
  addEventListener(
    type: 'message',
    listener: (event: { data: StoreWorkerRequest }) => void,
  ): void
}

let handle: EntityStoreHandle | null = null
// Init the WASM store, then post the readiness handshake so the host can decide
// worker-vs-in-process. A failure here (e.g. the webview can't run the worker)
// is reported as `ready: false` rather than left to hang.
const ready: Promise<void> = (async () => {
  try {
    const factory = await loadEntityStoreHandleFactory()
    handle = factory()
    ctx.postMessage({ type: 'ready', ok: true })
  } catch (error) {
    ctx.postMessage({
      type: 'ready',
      ok: false,
      error: error instanceof Error ? error.message : String(error),
    })
    throw error
  }
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
