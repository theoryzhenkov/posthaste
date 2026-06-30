import { describe, expect, it } from 'bun:test'

import type { EntityStoreHandle } from '../src/runtime/replica/handle'
import { resolveStorePort } from '../src/runtime/replica/storePortResolver'
import type { WorkerStorePort } from '../src/runtime/replica/workerStorePort'

// A minimal handle — `resolveStorePort` only wraps it in InProcessStorePort and
// never calls a method here, so an empty object suffices.
const fakeHandle = {} as EntityStoreHandle
const loadHandle = () => Promise.resolve(() => fakeHandle)

/** A stand-in WorkerStorePort exposing just what the resolver touches. */
function fakeWorker(ready: Promise<void>) {
  let terminated = false
  const port = {
    ready,
    terminate: () => {
      terminated = true
    },
  } as unknown as WorkerStorePort
  return { port, wasTerminated: () => terminated }
}

describe('resolveStorePort', () => {
  it('uses the in-process store when the worker is disabled (never creates one)', async () => {
    let created = false
    const result = await resolveStorePort({
      workerEnabled: false,
      createWorkerStorePort: () => {
        created = true
        return fakeWorker(Promise.resolve()).port
      },
      loadHandle,
    })
    expect(result.kind).toBe('in-process')
    expect(created).toBe(false)
  })

  it('uses the worker when it signals readiness', async () => {
    const worker = fakeWorker(Promise.resolve())
    const result = await resolveStorePort({
      workerEnabled: true,
      createWorkerStorePort: () => worker.port,
      loadHandle,
    })
    expect(result.kind).toBe('worker')
    expect(result.port).toBe(worker.port)
    expect(worker.wasTerminated()).toBe(false)
  })

  it('falls back + terminates the worker when readiness rejects (init failure)', async () => {
    const worker = fakeWorker(Promise.reject(new Error('wasm init failed')))
    let fallbackError: unknown
    const result = await resolveStorePort({
      workerEnabled: true,
      createWorkerStorePort: () => worker.port,
      loadHandle,
      onFallback: (error) => {
        fallbackError = error
      },
    })
    expect(result.kind).toBe('in-process')
    expect(worker.wasTerminated()).toBe(true)
    expect((fallbackError as Error).message).toContain('wasm init failed')
  })

  it('falls back + terminates the worker when readiness times out', async () => {
    // A worker that never resolves readiness; a tiny timeout forces the fallback.
    const worker = fakeWorker(new Promise<void>(() => {}))
    const result = await resolveStorePort({
      workerEnabled: true,
      createWorkerStorePort: () => worker.port,
      loadHandle,
      timeoutMs: 5,
    })
    expect(result.kind).toBe('in-process')
    expect(worker.wasTerminated()).toBe(true)
  })
})
