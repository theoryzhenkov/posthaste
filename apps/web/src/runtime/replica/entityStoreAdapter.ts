/**
 * The client-layer entity-store adapter (slice 2e): a `RuntimeAdapter` that
 * drives the normalized WASM `EntityStore` from the runtime frame stream, while
 * every other surface passes through to a base adapter. Unconditional (the
 * client's sole read model); the renderer is unchanged (`runtime-adapter-opaque`) —
 * it still consumes `viewSnapshot`/`viewReplace` frames, which this adapter
 * synthesizes from the store's projected rows.
 *
 * The store is the single derivation for the mail list: `message.updated`
 * notification frames (carrying the full `projection` + `countDeltas`, per 2c/2b)
 * feed `ingest_batch`, and the store self-maintains each evaluable view's
 * membership (place-or-ignore against the held coverage `[TOP, W]`). On each
 * drain the adapter re-projects open views + emits `viewReplace` for changed
 * ones (covers content-only mutations — P2 — without a message→rows index),
 * and writes the affected mailboxes' counts straight into the React Query
 * cache (`setQueryData`) so the sidebar updates without a REST refetch. Rows
 * and counts arrive on one stream, one batch — no divergence (I1).
 *
 * Views open **non-delta-capable** (option i): the runtime serves full
 * `viewSnapshot`/`viewReplace`, and the store re-derives placement from
 * `message.updated` rather than reconciling incremental `viewDelta` frames.
 *
 * @spec docs/eph/DESIGN-L2-client-link-reactive-store (2e.2)
 * @spec docs/eph/PLAN-L2-client-link-reactive-store (2e.2)
 */
import type { QueryClient } from '@tanstack/react-query'

import { queryClient as singletonQueryClient } from '@/app/queryClient'
import { LOG_EVENTS, syncLogger } from '@/logger'
import type { Mailbox } from '@/api/types'
import type { DomainEvent } from '@/api/types'
import { queryKeys } from '@/queryKeys'
import type {
  RuntimeAdapter,
  RuntimeFrame,
  RuntimeFrameHandlers,
  RuntimeFrameSubscriptionRequest,
  RuntimeMailListRowState,
  RuntimeMailListViewState,
  RuntimeMutationReceipt,
  RuntimeOpenMessageListViewResult,
  RuntimeRunMutationRequest,
  RuntimeLinkViewCloseRequest,
  RuntimeLinkViewExtendRequest,
  RuntimeLinkViewRequest,
  RuntimeUnsubscribe,
  RuntimeViewSnapshot,
} from '../types'
import type { OkResponse } from '../../api/types'
import type {
  EntityStoreHandleFactory,
  MessageChangeDiff,
  ReplicaAssertion,
  SettlementVerdict,
} from './handle'
import { InProcessStorePort, type StorePort } from './storePort'
import { settlementVerdict } from './mapping'
import {
  buildMailListPredicateContext,
  resolveMailListPredicate,
  type MailListPredicate,
} from '../mailListSelfMaintained'
import type { OutboxStore } from './outboxStore'
import { getUndoHistoryStore, makeRevStep } from './undoHistoryStore'
import { parseMailOperation } from './wasmUtil'
import {
  nearEndLinkId,
  setNearEndOutboxHooks,
  type NearEndOutboxHooks,
} from '../nearEnd'

/** The store's sort key `[receivedAt, id]`. */
interface SortKey {
  receivedAt: string
  messageId: string
}

/** The store's membership predicate (externally-tagged + camelCase on the wire). */
type ViewPredicate = MailListPredicate

/** A row the store places (`setViewRowsJson` input). */
interface StoreViewRow {
  rowKey: string
  messageId: string
  sortKey: SortKey
}

/** One authoritative update in a batch (`ingestBatchJson` input). */
type StoreUpdate =
  | {
      message: {
        messageId: string
        projection: unknown
        deleted: boolean
        countDeltas: CountDelta[]
      }
    }
  | { mailboxCount: CountDelta }

interface CountDelta {
  mailboxId: string
  unreadCount: number
  totalCount: number
}

/** A dirty key from `drainDirtyJson` (`{message|mailbox|view: id}`). */
type DirtyKey = { message: string } | { mailbox: string } | { view: string }

/** Cancels a scheduled coalesced flush. */
type FlushCanceller = () => void

/**
 * Schedules `cb` to run on the next frame, returning a canceller. The default
 * uses `requestAnimationFrame`; when it is unavailable (non-DOM / tests) it runs
 * `cb` synchronously so per-frame behavior is preserved without coalescing.
 */
type FlushScheduler = (cb: () => void) => FlushCanceller

function defaultFlushScheduler(cb: () => void): FlushCanceller {
  if (typeof requestAnimationFrame === 'function') {
    const id = requestAnimationFrame(() => cb())
    return () => cancelAnimationFrame(id)
  }
  cb()
  return () => {}
}

/**
 * Cap on buffered sync frames before a synchronous flush. Bounds the per-flush
 * batch (one projection covers at most this many ingests) and guarantees
 * forward progress + bounded memory if `requestAnimationFrame` is throttled
 * (e.g. a backgrounded tab).
 */
const SYNC_FLUSH_CAP = 256

type MailListFrame = RuntimeFrame<RuntimeMailListViewState>
type NotificationFrame = Extract<MailListFrame, { type: 'notification' }>

function isMessageUpdatedNotification(
  frame: MailListFrame,
): frame is NotificationFrame {
  return (
    frame.type === 'notification' &&
    (frame.payload as DomainEvent | undefined)?.topic === 'message.updated'
  )
}

export interface EntityStoreAdapterDeps {
  base: RuntimeAdapter
  /** Builds the in-process store. Required unless `makeStore` is given. */
  makeHandle?: EntityStoreHandleFactory
  /** Overrides the store entirely — e.g. a `WorkerStorePort` running the WASM
   *  store off the UI thread. Takes precedence over `makeHandle`. */
  makeStore?: () => StorePort
  outbox: OutboxStore
  /** React Query cache the adapter writes mailbox counts into. Defaults to the
   * app singleton. */
  queryClient?: QueryClient
  now?: () => number
  /** Coalesces the `message.updated` sync burst (default: one flush per
   * animation frame). Injected synchronously in tests. */
  scheduleFlush?: FlushScheduler
  /** The near-end engine surface the adapter wires its durable outbox into
   * (the engine's level-triggered reconciler drives replay + settlement,
   * D44). Defaults to the live wasm engine binding; injected in tests. */
  nearEnd?: {
    setOutboxHooks(hooks: NearEndOutboxHooks): void
    linkId(): string | null
  }
}

interface ViewEntry {
  /** The store predicate this view was registered with. */
  predicate: ViewPredicate
  lastSnapshot: RuntimeViewSnapshot<RuntimeMailListViewState>
  /** The last projected-rows JSON, to emit `viewReplace` only when it moved. */
  lastProjectionJson: string
}

function sortKeyOf(projection: { receivedAt: string; id: string }): SortKey {
  return { receivedAt: projection.receivedAt, messageId: projection.id }
}

/**
 * The watermark `W` — the sort key of the last held row. `null` (the range
 * reaches BOTTOM / complete) when the snapshot's coverage has no ranges.
 */
function watermarkFromSnapshot(
  snapshot: RuntimeViewSnapshot<RuntimeMailListViewState>,
  rows: RuntimeMailListRowState[],
): SortKey | null {
  if (!snapshot.coverage?.ranges?.length) {
    return null
  }
  const last = rows[rows.length - 1]
  return last ? sortKeyOf(last.projection) : null
}

/** Map a served row to the store's `ViewRow` (identity + position). */
function toStoreRow(row: RuntimeMailListRowState): StoreViewRow {
  return {
    rowKey: `${row.projection.sourceId}:${row.projection.id}`,
    messageId: row.projection.id,
    sortKey: sortKeyOf(row.projection),
  }
}

/** Seed message bases for every served row (P1: a row implies a live base). */
function projectionBatchFromRows(
  rows: RuntimeMailListRowState[],
): StoreUpdate[] {
  return rows.map((row) => ({
    message: {
      messageId: row.projection.id,
      projection: row.projection,
      deleted: false,
      countDeltas: [],
    },
  }))
}

class EntityStoreController {
  private readonly views = new Map<string, ViewEntry>()
  private readonly mailboxAccount = new Map<string, string>()
  private sink: RuntimeFrameHandlers | null = null
  private seq = 1_000_000
  private readonly now: () => number
  private readonly deps: EntityStoreAdapterDeps
  private readonly store: StorePort
  private readonly queryClient: QueryClient
  // Serializes store operations so a flush/snapshot/mutation/settle never
  // interleaves another's awaits — the correctness keystone once the store is
  // behind an async (eventually cross-thread) boundary.
  private storeQueue: Promise<unknown> = Promise.resolve()
  // Buffer of `message.updated` frames awaiting a coalesced flush, plus the
  // canceller for the scheduled flush (null when none is pending).
  private pendingFrames: NotificationFrame[] = []
  private cancelScheduledFlush: FlushCanceller | null = null
  private readonly scheduleFlush: FlushScheduler

  private readonly nearEnd: NonNullable<EntityStoreAdapterDeps['nearEnd']>

  constructor(deps: EntityStoreAdapterDeps) {
    this.deps = deps
    if (deps.makeStore) {
      this.store = deps.makeStore()
    } else if (deps.makeHandle) {
      this.store = new InProcessStorePort(deps.makeHandle())
    } else {
      throw new Error('entityStoreAdapter requires makeHandle or makeStore')
    }
    this.queryClient = deps.queryClient ?? singletonQueryClient
    this.now = deps.now ?? (() => Date.now())
    this.scheduleFlush = deps.scheduleFlush ?? defaultFlushScheduler
    this.nearEnd = deps.nearEnd ?? {
      setOutboxHooks: setNearEndOutboxHooks,
      linkId: nearEndLinkId,
    }
    // Wire the durable outbox into the engine's level-triggered reconciler
    // (D44): the engine decides WHEN (every connect); these hooks are HOW.
    this.nearEnd.setOutboxHooks(this.buildOutboxHooks())
  }

  /**
   * The engine's reconciler hooks over the durable outbox:
   *
   * - never-dispatched replay (D44a — subsumes and replaces the deleted
   *   view-open `resendNeverDispatched` trigger);
   * - sent-but-unsettled settlement (D44b — the cross-link query the old
   *   adapter left as a TODO): a terminal verdict settles the optimism +
   *   clears the record; a runtime with no record re-forwards.
   */
  private buildOutboxHooks(): NearEndOutboxHooks {
    return {
      neverDispatched: async () => {
        const records = await this.deps.outbox.all()
        return records
          .filter((record) => record.runtimeMutationId === null)
          .flatMap((record) => {
            if (record.request) {
              return [record.request]
            }
            syncLogger.warn(
              {
                event: LOG_EVENTS.outboxRehydrateSkipped,
                clientMutationId: record.clientMutationId,
              },
              'outbox record predates the stored request; cannot replay it',
            )
            return []
          })
      },
      onReconciled: async (receipt, linkId) => {
        if (!receipt.runtimeMutationId) {
          return
        }
        syncLogger.info(
          {
            event: LOG_EVENTS.outboxRehydrateResent,
            clientMutationId: receipt.clientMutationId,
          },
          'reconciler replayed an outbox record; linking its receipt',
        )
        await this.deps.outbox.linkRuntimeMutationId(
          receipt.clientMutationId,
          receipt.runtimeMutationId,
          linkId ?? undefined,
        )
      },
      sentUnsettled: async () => {
        const records = await this.deps.outbox.all()
        return records
          .filter(
            (record) => record.runtimeMutationId !== null && record.linkId,
          )
          .map((record) => ({
            linkId: record.linkId as string,
            clientMutationId: record.clientMutationId,
            ...(record.request ? { request: record.request } : {}),
          }))
      },
      onSettlement: async (receipt) => {
        // The runtime already settled it terminally in a prior link: settle
        // the optimism (a no-op when nothing is folded) and clear the record.
        await this.settleAll(
          receipt.clientMutationId,
          receipt.state === 'confirmed' ? 'confirmed' : 'failed',
        )
        await this.deps.outbox.remove(receipt.clientMutationId)
      },
    }
  }

  /**
   * Run `op` after all previously-enqueued store ops, so dependent store calls
   * (ingest → drain → project) stay atomic relative to other ops. Enqueued ops
   * must NOT call `enqueue` themselves (that would deadlock on the running op).
   */
  private enqueue<T>(op: () => Promise<T>): Promise<T> {
    const result = this.storeQueue.then(op, op)
    this.storeQueue = result.then(
      () => undefined,
      () => undefined,
    )
    return result
  }

  async openMailListView(
    request: RuntimeLinkViewRequest,
  ): Promise<RuntimeOpenMessageListViewResult> {
    // Option (i): the view opens non-delta-capable, so the runtime serves full
    // `viewSnapshot`/`viewReplace` and the store re-derives from `message.updated`.
    const result = await this.deps.base.openRuntimeLinkMessageListView(request)
    return this.enqueue(() => this.seedOpenedView(request, result))
  }

  /** Seed a freshly-opened view + rehydrate durable intent, as one store op. */
  private async seedOpenedView(
    request: RuntimeLinkViewRequest,
    result: RuntimeOpenMessageListViewResult,
  ): Promise<RuntimeOpenMessageListViewResult> {
    const { viewId, snapshot } = result
    const rows = snapshot.data.rows
    const predicate = resolveMailListPredicate(
      request.view.scope,
      request.view.sort,
      buildMailListPredicateContext(this.queryClient),
    )
    await this.store.registerViewJson(
      viewId,
      JSON.stringify({
        predicate,
        sortField: request.view.sort ?? 'date',
        sortDirection: request.view.sortDir ?? 'desc',
        watermark: watermarkFromSnapshot(snapshot, rows),
      }),
    )
    // P1: seed the rows' message bases + place the rows in one atomic batch.
    await this.store.ingestBatchJson(
      JSON.stringify(projectionBatchFromRows(rows)),
    )
    await this.store.setViewRowsJson(
      viewId,
      JSON.stringify(rows.map(toStoreRow)),
      JSON.stringify(watermarkFromSnapshot(snapshot, rows)),
    )
    await this.store.drainDirtyJson()
    // Rehydrate durable intent over the freshly-served base.
    //
    // A record already sent to the runtime (`runtimeMutationId !== null`) is
    // runtime-owned: the base served just above already reflects its outcome
    // when settled, and the near-end engine's reconciler resolves the
    // sent-but-unsettled remainder on every connect (D44b: query the runtime's
    // settlement by the stored link + client mutation id; settle locally or
    // re-forward). So it is NOT re-applied as pending intent here — and, when
    // it carries the link id the settlement query needs, it is KEPT for the
    // reconciler. A legacy sent record without one cannot be queried; drop it
    // (the pre-reconciler behavior, which also caps the old settled-record
    // leak). Never-sent records are re-folded below; the reconciler replays
    // them on connect (the view-open resend trigger is deleted, D44a).
    //
    // A single un-deserializable record (e.g. a durable assertion written before
    // a wire-schema change) must never abort the whole view-open — that bricks
    // every mail-list view silently. Skip + log the bad record and keep going.
    const rehydrated = await this.deps.outbox.all()
    const droppedLegacy: string[] = []
    for (const record of rehydrated) {
      if (record.runtimeMutationId !== null) {
        if (!record.linkId) {
          droppedLegacy.push(record.clientMutationId)
        }
        continue
      }
      try {
        await this.store.acceptMutationJson(
          JSON.stringify({
            mutationId: record.clientMutationId,
            messageId: record.messageId,
            assertion: record.assertion,
          }),
        )
      } catch (error) {
        syncLogger.error(
          {
            event: LOG_EVENTS.outboxRehydrateSkipped,
            clientMutationId: record.clientMutationId,
            messageId: record.messageId,
            error: error instanceof Error ? error.message : String(error),
          },
          'skipped an un-deserializable outbox record during rehydration',
        )
      }
    }
    if (droppedLegacy.length) {
      await Promise.all(droppedLegacy.map((id) => this.deps.outbox.remove(id)))
      syncLogger.debug(
        {
          event: LOG_EVENTS.outboxRehydrateDropped,
          count: droppedLegacy.length,
        },
        'dropped sent outbox records with no link id (not reconcilable)',
      )
    }
    const projected = await this.projectView(viewId)
    const entry: ViewEntry = {
      predicate,
      lastSnapshot: snapshot,
      lastProjectionJson: projected.json,
    }
    this.views.set(viewId, entry)
    return { viewId, snapshot: this.snapshotFrom(entry, projected.rows) }
  }

  /** Extend a tracked view's window. The base returns the extended snapshot,
   *  but the store must be re-seeded with it (mirrors the served-snapshot path
   *  in `onBaseFrame`) — otherwise a `message.updated` arriving before the
   *  broadcast `viewReplace` projects the pre-extend rows + emits a
   *  row-dropping `viewReplace` (the loadMore-vs-firehose race). The caller
   *  still applies the returned snapshot directly for scroll responsiveness.
   */
  async extendMailListView(
    request: RuntimeLinkViewExtendRequest,
  ): Promise<RuntimeOpenMessageListViewResult> {
    const result = await this.deps.base.extendRuntimeLinkView(request)
    const entry = this.views.get(result.viewId)
    if (!entry) {
      // Untracked view (not opened through the store): pass through unchanged.
      return result
    }
    const rows = result.snapshot.data.rows
    await this.enqueue(async () => {
      await this.store.ingestBatchJson(
        JSON.stringify(projectionBatchFromRows(rows)),
      )
      await this.store.setViewRowsJson(
        result.viewId,
        JSON.stringify(rows.map(toStoreRow)),
        JSON.stringify(watermarkFromSnapshot(result.snapshot, rows)),
      )
      entry.lastSnapshot = result.snapshot
      await this.store.drainDirtyJson()
      entry.lastProjectionJson = (await this.projectView(result.viewId)).json
    })
    return result
  }

  closeView(request: RuntimeLinkViewCloseRequest): Promise<OkResponse> {
    this.views.delete(request.viewId)
    void this.enqueue(() => this.store.closeView(request.viewId))
    return this.deps.base.closeRuntimeLinkView(request)
  }

  subscribe(
    request: RuntimeFrameSubscriptionRequest,
    handlers: RuntimeFrameHandlers,
  ): RuntimeUnsubscribe {
    this.sink = handlers
    const wrapped: RuntimeFrameHandlers = {
      ...handlers,
      onFrame: (frame) => this.routeFrame(frame, handlers),
      onReset: () => {
        // D49: the incremental view is broken. Drop buffered incremental frames
        // and any scheduled flush; the runtime re-serves whole snapshots over the
        // fresh subscription (handled by `onBaseFrame` → re-seed), so no stale
        // delta lingers. Then surface upward.
        this.pendingFrames = []
        if (this.cancelScheduledFlush) {
          this.cancelScheduledFlush()
          this.cancelScheduledFlush = null
        }
        handlers.onReset?.()
      },
    }
    const unsubscribe = this.deps.base.subscribeRuntimeFrames(request, wrapped)
    return () => {
      if (this.cancelScheduledFlush) {
        this.cancelScheduledFlush()
        this.cancelScheduledFlush = null
      }
      this.pendingFrames = []
      if (this.sink === handlers) {
        this.sink = null
      }
      unsubscribe()
    }
  }

  /**
   * Coalesce the `message.updated` sync burst. During a full re-sync (e.g. after
   * repair) the runtime streams thousands of per-message frames; processing
   * each one immediately re-projects the dirty views and re-renders the list
   * per message — O(events x rows) on the UI thread. Buffer them and apply one
   * batch per animation frame instead: ingest all, re-project once, forward
   * downstream within a single task (so React batches the renders).
   *
   * Other frames (view snapshots, mutation verdicts) are order-sensitive and
   * low-volume, so they flush the buffer first to preserve arrival order, then
   * apply immediately.
   */
  private routeFrame(
    frame: RuntimeFrame<RuntimeMailListViewState>,
    handlers: RuntimeFrameHandlers,
  ): void {
    if (isMessageUpdatedNotification(frame)) {
      this.pendingFrames.push(frame)
      if (this.pendingFrames.length >= SYNC_FLUSH_CAP) {
        this.flushPendingFrames(handlers)
      } else {
        this.ensureFlushScheduled(handlers)
      }
      return
    }
    this.flushPendingFrames(handlers)
    this.onBaseFrame(frame, handlers)
  }

  private ensureFlushScheduled(handlers: RuntimeFrameHandlers): void {
    if (this.cancelScheduledFlush) {
      return
    }
    let ranSynchronously = false
    const canceller = this.scheduleFlush(() => {
      ranSynchronously = true
      this.cancelScheduledFlush = null
      this.flushPendingFrames(handlers)
    })
    // A synchronous scheduler (tests / non-DOM) already flushed; nothing to hold.
    if (!ranSynchronously) {
      this.cancelScheduledFlush = canceller
    }
  }

  private flushPendingFrames(handlers: RuntimeFrameHandlers): void {
    if (this.cancelScheduledFlush) {
      this.cancelScheduledFlush()
      this.cancelScheduledFlush = null
    }
    if (this.pendingFrames.length === 0) {
      return
    }
    const frames = this.pendingFrames
    this.pendingFrames = []
    void this.enqueue(async () => {
      // P3: fold the whole burst into ONE store ingest (not one per frame) so a
      // coalesced flush is a single round-trip over a WorkerStorePort instead
      // of N. The mailbox→account tracking stays on the main thread.
      const updates: StoreUpdate[] = []
      for (const frame of frames) {
        const update = this.storeUpdateFromEvent(frame.payload as DomainEvent)
        if (update) updates.push(update)
      }
      if (updates.length) {
        await this.store.ingestBatchJson(JSON.stringify(updates))
      }
      await this.drainAndEmit()
      await this.clearRetired()
      for (const frame of frames) {
        handlers.onFrame(frame)
      }
    })
  }

  async runMutation(request: RuntimeRunMutationRequest) {
    const translated = await parseMailOperation(
      request,
      this.roleMapForRequest(request),
    )
    if (!translated) {
      return this.deps.base.runRuntimeMutation(request)
    }
    const clientMutationId = request.clientMutationId
    // Capture the invertible diff BEFORE folding the assertion (prev = current
    // folded base, curr = base + assertion), for the client-owned undo history.
    // `message.applyDiff` is the undo/redo vehicle itself — the hook navigates
    // the history for it, so it is NOT a forward action to record.
    const isUndoVehicle = request.name === 'message.applyDiff'
    // Phase 2 Slice 5d: only USER-initiated mutations (tagged `userInitiated` in
    // their context) record an undo step. Internal/side-effect mutations — e.g.
    // auto-mark-read, sync-induced re-projection — omit the tag, so they don't
    // pollute the undo history (an archive + a spurious setKeywords would
    // otherwise both be undoable, + the global undo would target the latest
    // spurious step instead of the archive). @spec Phase 2 Slice 5d
    const isUserInitiated =
      (request.context as { userInitiated?: boolean } | null | undefined)
        ?.userInitiated === true
    // Atomic optimism: capture the undo diff, fold the assertion, persist the
    // outbox record, and re-project — serialized against flushes/settles so the
    // fold never interleaves another store op.
    const capturedDiff = await this.enqueue(async () => {
      let diff: MessageChangeDiff | null = null
      if (!isUndoVehicle && isUserInitiated) {
        const diffJson = await this.store.captureMutationDiffJson(
          translated.messageId,
          JSON.stringify(translated.assertion),
        )
        if (diffJson !== 'null') {
          diff = JSON.parse(diffJson) as MessageChangeDiff
        }
      }
      await this.store.acceptMutationJson(
        JSON.stringify({
          mutationId: clientMutationId,
          messageId: translated.messageId,
          assertion: translated.assertion,
        }),
      )
      await this.deps.outbox.put({
        clientMutationId,
        messageId: translated.messageId,
        assertion: translated.assertion as ReplicaAssertion,
        runtimeMutationId: null,
        acceptedAt: this.now(),
        // Store the original send so a never-dispatched record can be replayed
        // verbatim on rehydration (outbox-rehydrate-resend).
        request,
      })
      await this.drainAndEmit()
      return diff
    })

    const receipt = await this.dispatchToRuntime(request, clientMutationId)
    // Record the forward action in the client history once the runtime has
    // accepted it. A persist failure here must NOT trigger the optimism revert
    // (the mutation succeeded), so this is outside the try/catch above.
    if (capturedDiff) {
      const sourceId =
        (request.args as { sourceId?: string } | undefined)?.sourceId ?? ''
      await getUndoHistoryStore().pushForward(
        sourceId,
        makeRevStep(translated.messageId, sourceId, capturedDiff),
      )
    }
    return receipt
  }

  /**
   * Send a translated mutation to the runtime and link/settle its receipt
   * against the durable outbox (stamping the dispatching link so the
   * engine's reconciler can query its settlement cross-link, D44b). On a
   * synchronous rejection it retires the optimism (revert) and rethrows.
   * Replay of never-dispatched records is NOT here anymore — the engine's
   * level-triggered reconciler owns it (D44a).
   */
  private async dispatchToRuntime(
    request: RuntimeRunMutationRequest,
    clientMutationId: string,
  ): Promise<RuntimeMutationReceipt> {
    let receipt: RuntimeMutationReceipt
    try {
      receipt = await this.deps.base.runRuntimeMutation(request)
      if (receipt.runtimeMutationId) {
        await this.deps.outbox.linkRuntimeMutationId(
          clientMutationId,
          receipt.runtimeMutationId,
          this.nearEnd.linkId() ?? request.linkId ?? undefined,
        )
      }
    } catch (error) {
      // Synchronous rejection: retire the optimism and surface the revert.
      await this.settleAll(clientMutationId, 'failed')
      throw error
    }
    return receipt
  }

  private onBaseFrame(
    frame: RuntimeFrame<RuntimeMailListViewState>,
    handlers: RuntimeFrameHandlers,
  ): void {
    switch (frame.type) {
      case 'viewSnapshot':
      case 'viewReplace': {
        const entry = this.views.get(frame.viewId)
        if (!entry) {
          handlers.onFrame(frame)
          return
        }
        // A served snapshot / page / resync: re-seed the rows' bases + place
        // them (P1), then re-project from the store.
        const rows = frame.snapshot.data.rows
        void this.enqueue(async () => {
          await this.store.ingestBatchJson(
            JSON.stringify(projectionBatchFromRows(rows)),
          )
          await this.store.setViewRowsJson(
            frame.viewId,
            JSON.stringify(rows.map(toStoreRow)),
            JSON.stringify(watermarkFromSnapshot(frame.snapshot, rows)),
          )
          entry.lastSnapshot = frame.snapshot
          await this.drainAndEmit()
          await this.clearRetired()
        })
        return
      }
      case 'notification': {
        const event = frame.payload as DomainEvent | undefined
        if (event?.topic === 'message.updated') {
          void this.enqueue(async () => {
            await this.ingestMessageEvent(event)
            await this.drainAndEmit()
            await this.clearRetired()
          })
        }
        // Pass through: `useDaemonEvents` still handles non-store invalidations
        // (conversations, tags, smart-mailboxes) until 2e.3 retires the
        // message-owned ones.
        handlers.onFrame(frame)
        return
      }
      case 'mutationNotification': {
        // The verdict for a named mutation. `confirmed` retires the op by
        // absorption (no revert — it never outruns the base into a revert);
        // `rejected` reverts the optimism and surfaces the error. Either way the
        // durable outbox record clears (the server has reached a terminal state).
        const verdict = settlementVerdict(frame.notification)
        if (frame.notification.type === 'rejected') {
          syncLogger.warn(
            {
              event: LOG_EVENTS.runtimeMutationRejected,
              clientMutationId: frame.clientMutationId,
              errorCode: frame.notification.error.code,
              retryable: frame.notification.error.retryable,
            },
            'runtime mutation rejected; reverting optimism',
          )
        }
        void this.settleAll(frame.clientMutationId, verdict)
        handlers.onFrame(frame)
        return
      }
      default:
        handlers.onFrame(frame)
    }
  }

  /** Ingest a `message.updated` event's projection + count deltas into the store. */
  /** Build a single store update from a `message.updated` event (or null if
   *  there's nothing to materialize), tracking mailbox→account on the side.
   *  Extracted so a coalesced flush can fold a whole burst into one ingest. */
  private storeUpdateFromEvent(event: DomainEvent): StoreUpdate | null {
    const inner = event.payload as
      | {
          messageId?: string
          projection?: unknown
          countDeltas?: CountDelta[]
          deleted?: boolean
        }
      | undefined
    const messageId = inner?.messageId
    if (typeof messageId !== 'string') {
      return null
    }
    const deleted = inner?.deleted === true
    const projection = inner?.projection ?? null
    const countDeltas = inner?.countDeltas ?? []
    // Not deleted + no projection: nothing to materialize (shouldn't happen —
    // 2c attaches the projection to every non-destroy event).
    if (!deleted && !projection) {
      return null
    }
    const accountId = event.accountId
    if (accountId) {
      for (const delta of countDeltas) {
        this.mailboxAccount.set(delta.mailboxId, accountId)
      }
    }
    return { message: { messageId, projection, deleted, countDeltas } }
  }

  private async ingestMessageEvent(event: DomainEvent): Promise<void> {
    const update = this.storeUpdateFromEvent(event)
    if (!update) {
      return
    }
    await this.store.ingestBatchJson(JSON.stringify([update]))
  }

  /** The account's role→mailbox-id map from the cached mailbox list, so role
   *  moves (archive/trash/restoreToInbox/moveToRole) resolve to a
   *  `replaceMailboxes` op the replica can hold through the unconfirmed window.
   *  Empty when the mailbox list isn't cached yet (rare — the sidebar loads it
   *  on mount) → role moves get no optimism and fall back to the server-only
   *  path; no regression. */
  private roleMapForRequest(
    request: RuntimeRunMutationRequest,
  ): Record<string, string> {
    const sourceId = (request.args as { sourceId?: string } | undefined)
      ?.sourceId
    if (!sourceId) return {}
    const mailboxes = this.queryClient.getQueryData<Mailbox[]>(
      queryKeys.mailboxes(sourceId),
    )
    const roleMap: Record<string, string> = {}
    for (const mailbox of mailboxes ?? []) {
      if (mailbox.role) roleMap[mailbox.role] = mailbox.id
    }
    return roleMap
  }

  private async settleAll(
    clientMutationId: string,
    verdict: SettlementVerdict,
  ): Promise<void> {
    await this.enqueue(async () => {
      await this.store.settle(clientMutationId, verdict)
      await this.clearRetired()
      await this.drainAndEmit()
    })
  }

  /** Clear durable-outbox records for ops the engine retired since the last
   *  drain (settle-confirm or base catch-up). An un-retired op stays durable so
   *  it survives a reload to be replayed. (outbox D) */
  private async clearRetired(): Promise<void> {
    const retired = JSON.parse(await this.store.drainRetiredJson()) as string[]
    if (retired.length) {
      await Promise.all(retired.map((id) => this.deps.outbox.remove(id)))
    }
  }

  /** Drain the store's dirty keys, re-project the dirty views, and write counts. */
  private async drainAndEmit(): Promise<void> {
    const dirty = JSON.parse(await this.store.drainDirtyJson()) as DirtyKey[]
    // Re-project ONLY the views the store flagged dirty. The store now marks a
    // view dirty on any change to a row it holds — including a content-only
    // flag/read toggle (via its message→views reverse index) — so the drained
    // `view` set is trustworthy and complete; no all-views scan per drain
    // (`adapter-reproject-all`). The JSON-diff gate in `emitChangedViews` stays
    // the safety net against a true no-op rederive.
    const dirtyViews = new Set<string>()
    const dirtyMailboxes: string[] = []
    for (const key of dirty) {
      if ('view' in key) {
        dirtyViews.add(key.view)
      } else if ('mailbox' in key) {
        dirtyMailboxes.push(key.mailbox)
      }
    }
    for (const mailboxId of dirtyMailboxes) {
      await this.writeMailboxCount(mailboxId)
    }
    await this.emitChangedViews(dirtyViews)
  }

  /** Emit a synthesized `viewReplace` for each dirty view whose projection moved. */
  private async emitChangedViews(dirtyViews: Set<string>): Promise<void> {
    if (!this.sink) {
      return
    }
    for (const viewId of dirtyViews) {
      const entry = this.views.get(viewId)
      if (!entry) {
        continue
      }
      const projected = await this.projectView(viewId)
      if (projected.json === entry.lastProjectionJson) {
        continue
      }
      entry.lastProjectionJson = projected.json
      const snapshot = this.snapshotFrom(entry, projected.rows)
      this.sink.onFrame({
        type: 'viewReplace',
        linkSeq: this.seq++,
        viewId,
        revision: entry.lastSnapshot.revision,
        snapshot,
      })
    }
  }

  /** Re-project a view from the store: the joined rows → renderer rows. */
  private async projectView(viewId: string): Promise<{
    json: string
    rows: RuntimeMailListRowState[]
  }> {
    const json = await this.store.projectViewJson(viewId)
    const projected = JSON.parse(json) as
      | { rowKey: string; projection: unknown }[]
      | null
    if (!projected) {
      return { json, rows: [] }
    }
    const rows: RuntimeMailListRowState[] = projected.map((p) => ({
      rowKey: p.rowKey,
      resourceRef: null,
      projection: p.projection as RuntimeMailListRowState['projection'],
      orderKey: '',
      // `sortKey`/`pendingMarkers` are unused by the mail-list renderer.
    }))
    return { json, rows }
  }

  /** Write a dirty mailbox's counts straight into the React Query cache. */
  private async writeMailboxCount(mailboxId: string): Promise<void> {
    const accountId = this.mailboxAccount.get(mailboxId)
    if (!accountId) {
      return
    }
    const counts = JSON.parse(await this.store.mailboxJson(mailboxId)) as {
      unreadCount: number
      totalCount: number
    } | null
    if (!counts) {
      return
    }
    this.queryClient.setQueryData<Mailbox[]>(
      queryKeys.mailboxes(accountId),
      (old) =>
        old?.map((mailbox) =>
          mailbox.id === mailboxId
            ? {
                ...mailbox,
                unreadEmails: counts.unreadCount,
                totalEmails: counts.totalCount,
              }
            : mailbox,
        ),
    )
  }

  private snapshotFrom(
    entry: ViewEntry,
    rows: RuntimeMailListRowState[],
  ): RuntimeViewSnapshot<RuntimeMailListViewState> {
    return {
      ...entry.lastSnapshot,
      data: { ...entry.lastSnapshot.data, rows },
    }
  }
}

/**
 * Build an entity-store adapter over a base adapter. Every method not concerned
 * with the mail-list store delegates to the base unchanged.
 */
export function createEntityStoreAdapter(
  deps: EntityStoreAdapterDeps,
): RuntimeAdapter {
  const controller = new EntityStoreController(deps)
  return {
    ...deps.base,
    openRuntimeLinkMessageListView: (request) =>
      controller.openMailListView(request),
    extendRuntimeLinkView: (request) => controller.extendMailListView(request),
    closeRuntimeLinkView: (request) => controller.closeView(request),
    subscribeRuntimeFrames: (request, handlers) =>
      controller.subscribe(request, handlers),
    runRuntimeMutation: (request) => controller.runMutation(request),
  }
}
