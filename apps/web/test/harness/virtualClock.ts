/**
 * The harness VIRTUAL TIME: a tick-driven clock the harness owns.
 *
 * The suite already fakes time three ways — a captured `scheduleFlush` (the
 * adapter's rAF coalescer, injected so a burst is applied on command, per
 * `entityStoreAdapter.test.ts`), a macrotask `tick` to drain the serialized
 * store queue, and small real `callTimeoutMs` deadlines for the worker
 * watchdog. This formalizes the first two into one clock:
 *
 *  - `scheduleFlush` — the injected coalescer seam. It CAPTURES the adapter's
 *    flush callback instead of running it (no implicit rAF), so a `message.updated`
 *    burst stays buffered until the harness decides to apply it.
 *  - `flush()` — run the captured coalesced flush (if any), then await the
 *    controller's serialized store queue (via `flushActiveEntityStore`) plus a
 *    macrotask so a `WorkerStorePort`'s loopback microtask chain fully settles.
 *    This is the deterministic "apply everything pending" handle.
 *  - `advance(ms)` — let real timers (the worker watchdog's `setTimeout`) fire,
 *    then drain. The watchdog is the one place real wall-clock is unavoidable;
 *    tests keep its deadline tiny.
 *
 * @spec docs/eph/RFC-L2-client-resilience.md#D119
 */
import { flushActiveEntityStore } from '../../src/runtime/replica/entityStoreAdapter'

const macrotask = (): Promise<void> =>
  new Promise<void>((resolve) => setTimeout(resolve, 0))

export interface VirtualClock {
  /** The `scheduleFlush` seam to pass into `createEntityStoreAdapter`. */
  scheduleFlush: (cb: () => void) => () => void
  /** Whether a coalesced flush is currently buffered (unapplied). */
  hasScheduledFlush(): boolean
  /** Apply any buffered coalesced flush, then drain the store queue. */
  flush(): Promise<void>
  /** Let real timers fire for `ms`, then drain. */
  advance(ms: number): Promise<void>
}

export function createVirtualClock(): VirtualClock {
  let scheduled: (() => void) | null = null

  const drain = async (): Promise<void> => {
    const cb = scheduled
    scheduled = null
    cb?.()
    await flushActiveEntityStore()
    // One macrotask so a WorkerStorePort's loopback (microtask) replies settle,
    // then drain again to absorb any follow-up store op they enqueued.
    await macrotask()
    await flushActiveEntityStore()
  }

  return {
    scheduleFlush: (cb) => {
      scheduled = cb
      return () => {
        if (scheduled === cb) scheduled = null
      }
    },
    hasScheduledFlush: () => scheduled !== null,
    flush: drain,
    async advance(ms) {
      await new Promise<void>((resolve) => setTimeout(resolve, ms))
      await drain()
    },
  }
}
