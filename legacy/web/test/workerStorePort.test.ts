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
  private errorListener: ((event: { message?: string }) => void) | null = null
  readonly received: StoreWorkerRequest[] = []
  terminated = false

  constructor(
    private readonly respond: (
      request: StoreWorkerRequest,
    ) => StoreWorkerResponse | Promise<StoreWorkerResponse>,
  ) {}

  addEventListener(
    type: 'message' | 'error',
    listener:
      | ((event: { data: StoreWorkerOutbound }) => void)
      | ((event: { message?: string }) => void),
  ): void {
    if (type === 'error') {
      this.errorListener = listener as (event: { message?: string }) => void
    } else {
      this.listener = listener as (event: { data: StoreWorkerOutbound }) => void
    }
  }

  postMessage(request: StoreWorkerRequest): void {
    this.received.push(request)
    void Promise.resolve(this.respond(request)).then((response) =>
      this.listener?.({ data: response }),
    )
  }

  terminate(): void {
    this.terminated = true
  }

  /** Simulate the worker's readiness handshake. */
  emitReady(message: StoreWorkerOutbound): void {
    this.listener?.({ data: message })
  }

  /** Simulate the worker thread dying (the `error` event). */
  emitError(message: string): void {
    this.errorListener?.({ message })
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

  it('rejects in-flight + future calls when the worker dies after init (no hang)', async () => {
    // A worker that never answers, so the calls stay in-flight until it dies.
    const worker = new LoopbackWorker(
      () => new Promise<StoreWorkerResponse>(() => {}),
    )
    const port = new WorkerStorePort(worker)
    const inflight = port.drainDirtyJson()
    worker.emitError('worker crashed')
    // The in-flight call rejects (instead of hanging the controller's queue)...
    await expect(inflight).rejects.toThrow(/replica worker error/)
    // ...and any subsequent call fails fast too.
    await expect(port.projectViewJson('v1')).rejects.toThrow(
      /no longer available/,
    )
  })

  it('terminate() tears down the worker and fails outstanding calls', async () => {
    const worker = new LoopbackWorker(
      () => new Promise<StoreWorkerResponse>(() => {}),
    )
    const port = new WorkerStorePort(worker)
    const inflight = port.drainDirtyJson()
    port.terminate()
    expect(worker.terminated).toBe(true)
    await expect(inflight).rejects.toThrow(/terminated/)
  })

  it('a wedged worker (never replies) times out, restarts, and replays the request (W2 watchdog)', async () => {
    let spawnCount = 0
    const workers: LoopbackWorker[] = []
    const spawnWorker = () => {
      spawnCount += 1
      const isFirst = spawnCount === 1
      const worker = new LoopbackWorker((request) =>
        isFirst
          ? // The first worker never answers — wedged.
            new Promise<StoreWorkerResponse>(() => {})
          : { id: request.id, ok: true, result: 'recovered' },
      )
      workers.push(worker)
      return worker
    }
    const port = new WorkerStorePort(spawnWorker(), {
      spawnWorker,
      callTimeoutMs: 20,
      maxRestarts: 1,
    })

    const result = await port.projectViewJson('v1')

    expect(result).toBe('recovered')
    expect(spawnCount).toBe(2)
    expect(workers[0]?.terminated).toBe(true)
  })

  it('runs the re-seed hook on the fresh worker BEFORE replaying the timed-out call (CL-C1)', async () => {
    let spawnCount = 0
    const workers: LoopbackWorker[] = []
    const spawnWorker = () => {
      spawnCount += 1
      const isFirst = spawnCount === 1
      const worker = new LoopbackWorker((request) =>
        isFirst
          ? // The first worker wedges on the timed-out call.
            new Promise<StoreWorkerResponse>(() => {})
          : { id: request.id, ok: true, result: 'ok' },
      )
      workers.push(worker)
      return worker
    }
    const port = new WorkerStorePort(spawnWorker(), {
      spawnWorker,
      callTimeoutMs: 20,
      maxRestarts: 1,
    })

    // The controller's re-seed: it drives its own calls (registerViewJson) on
    // the fresh worker. Record the ORDER of methods the fresh worker sees to
    // prove the re-seed lands before the replayed call.
    const reseedCalls: string[] = []
    port.setReseedHook(async () => {
      await port.registerViewJson('v1', '{"sortField":"date"}')
      reseedCalls.push('reseed:registerViewJson')
    })

    // projectViewJson wedges worker #1 → respawn → re-seed → replay on worker #2.
    const result = await port.projectViewJson('v1')

    expect(result).toBe('ok')
    expect(spawnCount).toBe(2)
    // The fresh (2nd) worker saw the re-seed's registerViewJson FIRST, then the
    // replayed projectViewJson — never a replay into an un-seeded store.
    expect(workers[1]?.received.map((r) => r.method)).toEqual([
      'registerViewJson',
      'projectViewJson',
    ])
    expect(reseedCalls).toEqual(['reseed:registerViewJson'])
  })

  it('fails the port cleanly if the re-seed itself times out (fresh worker also wedged) — never a replay into emptiness (CL-C1)', async () => {
    let spawnCount = 0
    const spawnWorker = () => {
      spawnCount += 1
      // Every worker wedges — the initial call AND the re-seed's calls.
      return new LoopbackWorker(
        () => new Promise<StoreWorkerResponse>(() => {}),
      )
    }
    const port = new WorkerStorePort(spawnWorker(), {
      spawnWorker,
      callTimeoutMs: 20,
      maxRestarts: 1,
    })
    port.setReseedHook(async () => {
      // Times out on the fresh (also-wedged) worker → fails the port.
      await port.registerViewJson('v1', '{}')
    })

    // The original call rejects rather than resolving against an empty store or
    // respawn-looping.
    await expect(port.projectViewJson('v1')).rejects.toThrow()
    // Initial + one respawn; the re-seed's timeout latched the port dead, so no
    // further respawn churned.
    expect(spawnCount).toBe(2)
    await expect(port.drainDirtyJson()).rejects.toThrow(/no longer available/)
  })

  it('fails (not hangs) once the restart budget is exhausted, and fails fast afterward', async () => {
    let spawnCount = 0
    const spawnWorker = () => {
      spawnCount += 1
      return new LoopbackWorker(
        () => new Promise<StoreWorkerResponse>(() => {}),
      )
    }
    const port = new WorkerStorePort(spawnWorker(), {
      spawnWorker,
      callTimeoutMs: 10,
      maxRestarts: 2,
    })

    await expect(port.projectViewJson('v1')).rejects.toThrow(/timed out/)
    // The initial worker + 2 restarts = 3 spawns total.
    expect(spawnCount).toBe(3)
    // The port gave up permanently; further calls fail immediately rather
    // than hang or trigger more restarts.
    await expect(port.drainDirtyJson()).rejects.toThrow(/no longer available/)
  })

  it('without a spawnWorker option, a call timeout fails the port instead of hanging forever', async () => {
    const worker = new LoopbackWorker(
      () => new Promise<StoreWorkerResponse>(() => {}),
    )
    const port = new WorkerStorePort(worker, { callTimeoutMs: 10 })

    await expect(port.projectViewJson('v1')).rejects.toThrow(/timed out/)
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
