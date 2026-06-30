import { describe, expect, it } from 'bun:test'

import {
  WorkerStorePort,
  type ReplicaWorkerLike,
  type StoreWorkerOutbound,
  type StoreWorkerRequest,
  type StoreWorkerResponse,
} from '../src/runtime/replica/workerStorePort'

/** A loopback "worker": responses are produced by `respond` and delivered
 *  asynchronously, like a real `Worker`. Can also emit the readiness handshake. */
class LoopbackWorker implements ReplicaWorkerLike {
  private listener: ((event: { data: StoreWorkerOutbound }) => void) | null =
    null
  readonly received: StoreWorkerRequest[] = []

  constructor(
    private readonly respond: (
      request: StoreWorkerRequest,
    ) => StoreWorkerResponse | Promise<StoreWorkerResponse>,
  ) {}

  addEventListener(
    _type: 'message',
    listener: (event: { data: StoreWorkerOutbound }) => void,
  ): void {
    this.listener = listener
  }

  postMessage(request: StoreWorkerRequest): void {
    this.received.push(request)
    void Promise.resolve(this.respond(request)).then((response) =>
      this.listener?.({ data: response }),
    )
  }

  /** Simulate the worker's readiness handshake. */
  emitReady(message: StoreWorkerOutbound): void {
    this.listener?.({ data: message })
  }
}

describe('WorkerStorePort', () => {
  it('resolves `ready` on the worker readiness handshake', async () => {
    const worker = new LoopbackWorker((request) => ({
      id: request.id,
      ok: true,
      result: undefined,
    }))
    const port = new WorkerStorePort(worker)
    worker.emitReady({ type: 'ready', ok: true })
    await expect(port.ready).resolves.toBeUndefined()
  })

  it('rejects `ready` when the worker reports an init failure (drives in-process fallback)', async () => {
    const worker = new LoopbackWorker((request) => ({
      id: request.id,
      ok: true,
      result: undefined,
    }))
    const port = new WorkerStorePort(worker)
    worker.emitReady({ type: 'ready', ok: false, error: 'wasm init failed' })
    await expect(port.ready).rejects.toThrow('wasm init failed')
  })

  it('routes a call to the worker with its method + args and resolves the result', async () => {
    const worker = new LoopbackWorker((request) => ({
      id: request.id,
      ok: true,
      result:
        request.method === 'projectViewJson' ? '[{"rowKey":"r1"}]' : undefined,
    }))
    const port = new WorkerStorePort(worker)

    await port.registerViewJson('v1', '{"sortField":"date"}')
    expect(worker.received[0]).toEqual({
      id: 1,
      method: 'registerViewJson',
      args: ['v1', '{"sortField":"date"}'],
    })

    expect(await port.projectViewJson('v1')).toBe('[{"rowKey":"r1"}]')
  })

  it('rejects when the worker reports an error', async () => {
    const worker = new LoopbackWorker((request) => ({
      id: request.id,
      ok: false,
      error: 'boom',
    }))
    const port = new WorkerStorePort(worker)
    await expect(port.ingestBatchJson('[]')).rejects.toThrow('boom')
  })

  it('matches responses to calls by id even when they return out of order', async () => {
    const deferred: { id: number; settle: () => void }[] = []
    const worker = new LoopbackWorker(
      (request) =>
        new Promise<StoreWorkerResponse>((resolve) => {
          deferred.push({
            id: request.id,
            settle: () =>
              resolve({ id: request.id, ok: true, result: `r${request.id}` }),
          })
        }),
    )
    const port = new WorkerStorePort(worker)

    const first = port.drainDirtyJson()
    const second = port.drainRetiredJson()
    // Resolve the SECOND call's response first, then the first's.
    deferred[1].settle()
    deferred[0].settle()

    expect(await first).toBe('r1')
    expect(await second).toBe('r2')
  })
})
