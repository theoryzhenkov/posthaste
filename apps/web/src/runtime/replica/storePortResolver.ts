/**
 * Pick the replica store: a worker store if it initializes within the timeout,
 * else the in-process store.
 *
 * This is what makes worker-by-default SAFE on webviews that can't run the
 * worker (the Tauri WKWebView/WebView2 targets, where module-worker +
 * WASM-in-worker + `new Worker(new URL(...))` asset resolution can't be
 * validated from CI). A worker that never signals readiness, signals an init
 * failure, or times out is torn down and the host falls back to the in-process
 * store — so defaulting the worker on can't break the mail list anywhere.
 *
 * Extracted from the adapter install so the probe-or-fall-back decision is unit
 * testable without the module-load side effects of `adapter.ts`.
 *
 * @spec docs/eph/DESIGN-L2-replica-worker-isolation
 */
import type { EntityStoreHandleFactory } from './handle'
import { InProcessStorePort, type StorePort } from './storePort'
import type { WorkerStorePort } from './workerStorePort'

/** How long to wait for the worker's readiness handshake before falling back. */
export const WORKER_READY_TIMEOUT_MS = 5000

export interface ResolveStorePortDeps {
  /** False forces the in-process store (the `VITE_REPLICA_WORKER=false` path). */
  workerEnabled: boolean
  createWorkerStorePort: () => WorkerStorePort
  loadHandle: () => Promise<EntityStoreHandleFactory>
  /** Override the readiness timeout (tests). */
  timeoutMs?: number
  /** Notified (with the cause) when a probed worker is rejected for in-process. */
  onFallback?: (error: unknown) => void
}

export interface ResolvedStorePort {
  port: StorePort
  kind: 'worker' | 'in-process'
}

async function inProcess(
  loadHandle: () => Promise<EntityStoreHandleFactory>,
): Promise<ResolvedStorePort> {
  const makeHandle = await loadHandle()
  return { port: new InProcessStorePort(makeHandle()), kind: 'in-process' }
}

export async function resolveStorePort(
  deps: ResolveStorePortDeps,
): Promise<ResolvedStorePort> {
  if (!deps.workerEnabled) {
    return inProcess(deps.loadHandle)
  }
  const timeoutMs = deps.timeoutMs ?? WORKER_READY_TIMEOUT_MS
  let timer: ReturnType<typeof setTimeout> | undefined
  let port: WorkerStorePort | undefined
  try {
    port = deps.createWorkerStorePort()
    await Promise.race([
      port.ready,
      new Promise<never>((_, reject) => {
        timer = setTimeout(
          () => reject(new Error('worker readiness timed out')),
          timeoutMs,
        )
      }),
    ])
    return { port, kind: 'worker' }
  } catch (error) {
    deps.onFallback?.(error)
    // The probed worker is unusable; release it so it doesn't dangle.
    port?.terminate()
    return inProcess(deps.loadHandle)
  } finally {
    if (timer) {
      clearTimeout(timer)
    }
  }
}
