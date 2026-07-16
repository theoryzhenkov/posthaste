/**
 * `createClientHarness()` — the deterministic client testkit (the client twin
 * of posthaste-testkit, D119).
 *
 * It composes the three fakes into ONE driveable client:
 *
 *  (a) FAKE TRANSPORT  — {@link createFakeTransport}, the controllable frame
 *      stream behind the adapter's `base` seam (emit/sever/gap/reconnect);
 *  (b) FAKE WORKER     — {@link createWorkerKit}, a real `WorkerStorePort` over
 *      a loopback worker running the real wasm store (wedge/die); optional —
 *      the default store is the in-process real handle (deterministic, no
 *      watchdog timers) and `store: 'worker'` swaps in the wedgeable port;
 *  (c) VIRTUAL TIME    — {@link createVirtualClock}, the captured coalescer +
 *      the store-queue drain (`flush`) + the watchdog tick (`advance`).
 *
 * ...wired to the REAL entity-store adapter (the shipped wasm store) and a
 * react-query client, and returns the flattened drive handles the RFC names:
 * `emitFrame`, `severLink`, `wedgeWorker`, `advance`, `flush`.
 *
 * The pending set is the in-memory `MemoryPendingSetStore` and the near-end is
 * a stub that captures the durable-pending-set hooks (the engine's reconcile
 * TIMING is pinned by `nearEndEngine.test.ts` over fake IO; here the hooks are
 * driveable directly).
 *
 * @spec docs/eph/RFC-L2-client-resilience.md#D119
 */
import { QueryClient } from '@tanstack/react-query'

import type { Mailbox } from '../../src/api/types'
import { __resetLiveStoreForTesting } from '../../src/live-store/store'
import { queryKeys } from '../../src/queryKeys'
import {
  createEntityStoreAdapter,
  flushActiveEntityStore,
} from '../../src/runtime/replica/entityStoreAdapter'
import type { EntityStoreHandleFactory } from '../../src/runtime/replica/handle'
import { MemoryPendingSetStore } from '../../src/runtime/replica/pendingSetStore'
import type { PendingSetStore } from '../../src/runtime/replica/pendingSetStore'
import type { StorePort } from '../../src/runtime/replica/storePort'
import type {
  RuntimeAdapter,
  RuntimeFrame,
  RuntimeLinkViewRequest,
  RuntimeMailListViewState,
  RuntimeOpenMessageListViewResult,
} from '../../src/runtime/types'
import type { NearEndPendingSetHooks } from '../../src/runtime/nearEnd'

import {
  createFakeTransport,
  type FakeTransport,
  type FakeTransportOptions,
} from './fakeTransport'
import { createWorkerKit, type WorkerKit } from './fakeWorker'
import { loadRealHandleFactory } from './wasmHandle'
import { createVirtualClock, type VirtualClock } from './virtualClock'

const DEFAULT_VIEW_REQUEST: RuntimeLinkViewRequest = {
  linkId: 'sess',
  view: {
    scope: { kind: 'source-mailbox', sourceId: 's', mailboxId: 'inbox' },
    limit: 50,
    sort: 'date',
    sortDir: 'desc',
    operation: { name: 'test' } as never,
  },
}

export interface ClientHarnessOptions extends FakeTransportOptions {
  /** Store implementation behind the adapter. `in-process` (default) uses the
   *  real handle directly; `worker` uses a wedgeable `WorkerStorePort`. */
  store?: 'in-process' | 'worker'
  /** Seed the sidebar mailbox-structure cache (so count writes have a row). */
  mailboxes?: Mailbox[]
  /** Watchdog deadline for the worker store (only used with `store: 'worker'`). */
  callTimeoutMs?: number
  /** Watchdog restart budget for the worker store. */
  maxRestarts?: number
  /**
   * How the `message.updated` coalescer runs. `captured` (default) hands the
   * flush to the virtual clock so a burst stays buffered until `flush()`;
   * `synchronous` uses the adapter's default (non-DOM) scheduler that applies
   * each frame immediately onto the store queue — the shape the W3 unload-flush
   * path relies on (`flushActiveEntityStore` awaits the queue, not the buffer).
   */
  coalescer?: 'captured' | 'synchronous'
}

export interface ClientHarness {
  adapter: RuntimeAdapter
  queryClient: QueryClient
  pendingSet: PendingSetStore
  transport: FakeTransport
  /** The worker kit, present only when `store: 'worker'`. */
  worker?: WorkerKit
  clock: VirtualClock
  /** Frames the client layer emitted to the renderer sink. */
  frames: RuntimeFrame<RuntimeMailListViewState>[]
  /** Lifecycle signals the client layer surfaced to the renderer sink (what the
   *  shim/adapter expose for the F1/D49 failure modes). */
  signals: {
    permanentErrors: unknown[]
    transientErrors: unknown[]
    resets: number
    malformed: { raw: string; error: unknown }[]
  }
  /** The captured durable-pending-set hooks (the engine's reconcile surface). */
  pendingSetHooks(): NearEndPendingSetHooks
  /** Open the default mail-list view (seeds the store from the served snapshot). */
  openView(
    request?: RuntimeLinkViewRequest,
  ): Promise<RuntimeOpenMessageListViewResult>

  // --- flattened drive handles (the RFC's named surface) ---
  emitFrame: FakeTransport['emitFrame']
  severLink: FakeTransport['severLink']
  severWith: FakeTransport['severWith']
  gapFrame: FakeTransport['gapFrame']
  reconnect: FakeTransport['reconnect']
  /** Wedge the store worker (only meaningful with `store: 'worker'`). */
  wedgeWorker(): void
  advance: VirtualClock['advance']
  flush: VirtualClock['flush']

  dispose(): void
}

const DEFAULT_MAILBOXES: Mailbox[] = [
  {
    id: 'inbox',
    name: 'Inbox',
    role: 'inbox',
    unreadEmails: 2,
    totalEmails: 2,
  } as Mailbox,
]

export async function createClientHarness(
  options: ClientHarnessOptions = {},
): Promise<ClientHarness> {
  // The live store is a module singleton; reset its slices so a prior harness's
  // mirrored counts/projections can't bleed into this one.
  __resetLiveStoreForTesting()
  const transport = createFakeTransport(options)
  const clock = createVirtualClock()
  const queryClient = new QueryClient()
  queryClient.setQueryData<Mailbox[]>(
    queryKeys.mailboxes('s'),
    options.mailboxes ?? DEFAULT_MAILBOXES,
  )
  const pendingSet = new MemoryPendingSetStore()

  let worker: WorkerKit | undefined
  let makeStore: (() => StorePort) | undefined
  let makeHandle: EntityStoreHandleFactory | undefined
  if (options.store === 'worker') {
    worker = await createWorkerKit({
      ...(options.callTimeoutMs !== undefined
        ? { callTimeoutMs: options.callTimeoutMs }
        : {}),
      ...(options.maxRestarts !== undefined
        ? { maxRestarts: options.maxRestarts }
        : {}),
    })
    const port = worker.port
    makeStore = () => port
  } else {
    const factory = await loadRealHandleFactory()
    makeHandle = factory
  }

  let capturedHooks: NearEndPendingSetHooks | null = null

  const adapter = createEntityStoreAdapter({
    base: transport.base,
    ...(makeStore ? { makeStore } : {}),
    ...(makeHandle ? { makeHandle } : {}),
    pendingSet,
    queryClient,
    now: () => 1,
    // `synchronous` applies each frame immediately onto the store queue (the
    // shape `flushActiveEntityStore` — which awaits the queue, not the buffer —
    // relies on); an explicit scheduler beats the default's rAF under a DOM.
    scheduleFlush:
      options.coalescer === 'synchronous'
        ? (cb) => {
            cb()
            return () => {}
          }
        : clock.scheduleFlush,
    nearEnd: {
      setPendingSetHooks: (hooks) => {
        capturedHooks = hooks
      },
      linkId: () => 'sess-live',
    },
  })

  const frames: RuntimeFrame<RuntimeMailListViewState>[] = []
  const signals = {
    permanentErrors: [] as unknown[],
    transientErrors: [] as unknown[],
    resets: 0,
    malformed: [] as { raw: string; error: unknown }[],
  }
  const unsubscribe = adapter.subscribeRuntimeFrames(
    { linkId: 'sess' },
    {
      onFrame: (frame) => frames.push(frame),
      onPermanentError: (error) => signals.permanentErrors.push(error),
      onTransientError: (error) => signals.transientErrors.push(error),
      onReset: () => {
        signals.resets += 1
      },
      onMalformedFrame: (input) => signals.malformed.push(input),
    },
  )

  return {
    adapter,
    queryClient,
    pendingSet,
    transport,
    worker,
    clock,
    frames,
    signals,
    pendingSetHooks: () => {
      if (!capturedHooks) {
        throw new Error('pending-set hooks were not registered')
      }
      return capturedHooks
    },
    openView: (request = DEFAULT_VIEW_REQUEST) =>
      adapter.openRuntimeLinkMessageListView(request),

    emitFrame: transport.emitFrame,
    severLink: transport.severLink,
    severWith: transport.severWith,
    gapFrame: transport.gapFrame,
    reconnect: transport.reconnect,
    wedgeWorker: () => {
      if (!worker) {
        throw new Error(
          "wedgeWorker() requires store: 'worker' (the in-process store has no worker)",
        )
      }
      worker.wedge()
    },
    advance: clock.advance,
    flush: clock.flush,

    dispose: () => {
      unsubscribe()
      queryClient.clear()
      worker?.port.terminate()
      // Leave no dangling active-flush pointer from this adapter.
      void flushActiveEntityStore()
    },
  }
}
