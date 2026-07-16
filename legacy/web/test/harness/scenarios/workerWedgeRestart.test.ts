/**
 * Scenario (b) — F3 / M31: a wedged store worker is watchdog-restarted and the
 * timed-out call replayed.
 *
 * Re-expresses the M31 watchdog fix (previously `workerStorePort.test.ts`'s
 * hand-rolled loopback) through the unified harness fake worker. The port is
 * the REAL `WorkerStorePort`; only the worker across the `postMessage` boundary
 * is faked. Pins: a wedged worker (never replies) times out, is terminated +
 * respawned, and the SAME call is replayed on the fresh worker — the caller
 * sees a slow-but-answered call, not a hang. Also pins the F4 dead-latch: a
 * post-restart-budget failure and a crash `error` event fail fast.
 *
 * This kit wires NO re-seed hook (the port is driven bare), so it asserts the
 * PORT-level guarantee (the seam M31 fixed): the timed-out call is replayed to
 * completion on the fresh worker. Full store recovery — re-register views +
 * re-fold the pending set on respawn (CL-C1) — is now landed and is exercised
 * end-to-end in `workerRespawnReseed.test.ts` (full harness) and at the port
 * seam in `workerStorePort.test.ts` (the re-seed hook).
 *
 * @spec docs/eph/RFC-L2-client-resilience.md (F3, M31 / F4)
 */
import { describe, expect, it } from 'bun:test'

import { createWorkerKit } from '../index'

describe('scenario F3/M31: wedged worker → watchdog restart → replay', () => {
  it('times out a wedged worker, respawns, and replays the call to completion', async () => {
    // First worker wedges; the replacement answers (the classic M31 shape).
    const kit = await createWorkerKit({
      callTimeoutMs: 20,
      maxRestarts: 1,
      responderForSpawn: (spawn) => (request) =>
        spawn === 1
          ? new Promise(() => {}) // never answers — wedged
          : { id: request.id, ok: true, result: '[]' },
    })

    // drainDirtyJson on the wedged worker never replies → watchdog restarts →
    // the replayed call resolves on the fresh worker.
    const result = await kit.port.drainDirtyJson()

    expect(result).toBe('[]')
    expect(kit.spawnCount()).toBe(2)
    expect(kit.terminatedCount()).toBe(1) // the wedged one was torn down
  })

  it('recovers a real store call through the restart (real handle on both workers)', async () => {
    // Both workers run the REAL wasm handle; wedge the first via the drive
    // handle mid-call, so the watchdog restarts to a fresh real handle whose
    // drainDirtyJson answers deterministically.
    const kit = await createWorkerKit({ callTimeoutMs: 20, maxRestarts: 1 })
    kit.wedge()

    const result = await kit.port.drainDirtyJson()

    // A fresh real handle has no dirty keys → "[]"; the call completed rather
    // than hanging the store queue.
    expect(result).toBe('[]')
    expect(kit.spawnCount()).toBe(2)
  })

  it('fails fast once the restart budget is exhausted (bounded, no infinite respawn)', async () => {
    const kit = await createWorkerKit({
      callTimeoutMs: 10,
      maxRestarts: 2,
      responderForSpawn: () => () => new Promise(() => {}), // every worker wedges
    })

    await expect(kit.port.drainDirtyJson()).rejects.toThrow(/timed out/)
    // initial + 2 restarts = 3 spawns.
    expect(kit.spawnCount()).toBe(3)
    // The port gave up permanently; further calls fail immediately.
    await expect(kit.port.projectViewJson('v1')).rejects.toThrow(
      /no longer available/,
    )
  })

  it('a crash error event latches the port dead (F4): in-flight + future calls reject', async () => {
    const kit = await createWorkerKit({
      responderForSpawn: () => () => new Promise(() => {}), // never answers
    })
    const inflight = kit.port.drainDirtyJson()
    kit.die('worker crashed')

    await expect(inflight).rejects.toThrow(/replica worker error/)
    await expect(kit.port.projectViewJson('v1')).rejects.toThrow(
      /no longer available/,
    )
  })
})
