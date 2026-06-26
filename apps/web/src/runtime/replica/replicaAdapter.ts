/**
 * The client-layer replica adapter (W4): a `RuntimeAdapter` that produces a
 * mail-list view's frames locally from the WASM replica handle, while every
 * other surface passes through to a base adapter. Selected behind
 * `VITE_RUNTIME_REPLICA`; the renderer is unchanged (`runtime-adapter-opaque`).
 *
 * Optimism is instant: a message mutation folds into the open view handles and
 * a synthesized `viewReplace` is emitted before the network round-trip
 * resolves. The runtime's served bases (down-channel) replace the handle base
 * (keeping pending), and `mutationSettlement` retires or reverts the pending op.
 *
 * @spec docs/replication/client-link/L2#6-the-replicaadapter
 */
import type {
  RuntimeAdapter,
  RuntimeFrame,
  RuntimeFrameHandlers,
  RuntimeFrameSubscriptionRequest,
  RuntimeMailListDelta,
  RuntimeMailListRowState,
  RuntimeMailListViewState,
  RuntimeOpenMessageListViewResult,
  RuntimeRunMutationRequest,
  RuntimeSessionViewCloseRequest,
  RuntimeSessionViewRequest,
  RuntimeUnsubscribe,
  RuntimeViewSnapshot,
} from '../types'
import type { OkResponse } from '../../api/types'
import type { ReplicaHandle, ReplicaHandleFactory } from './handle'
import {
  applyOptimisticRows,
  membershipMailbox,
  replicaRowsFromViewState,
  settlementVerdict,
} from './mapping'
import type { OutboxStore } from './outboxStore'
import { parseMessageMutation } from './wasmUtil'

export interface ReplicaAdapterDeps {
  base: RuntimeAdapter
  makeHandle: ReplicaHandleFactory
  outbox: OutboxStore
  now?: () => number
}

interface ViewEntry {
  handle: ReplicaHandle
  membershipMailbox: string | null
  lastSnapshot: RuntimeViewSnapshot<RuntimeMailListViewState>
  lastProjectionJson: string
}

/**
 * Reconcile a runtime mail-list delta (replication client-link) into a served row set:
 * when `order` is present, reorder to it and drop rows whose key is absent;
 * then apply `upserts` by `rowKey`. Produces the same served base a whole
 * `viewReplace` would, which the replica then re-ingests (keeping pending).
 */
function applyRuntimeDelta(
  rows: RuntimeMailListRowState[],
  delta: RuntimeMailListDelta,
): RuntimeMailListRowState[] {
  const upsertByKey = new Map(delta.upserts.map((row) => [row.rowKey, row]))
  if (delta.order) {
    const heldByKey = new Map(rows.map((row) => [row.rowKey, row]))
    return delta.order
      .map((key) => upsertByKey.get(key) ?? heldByKey.get(key))
      .filter((row): row is RuntimeMailListRowState => row != null)
  }
  return rows.map((row) => upsertByKey.get(row.rowKey) ?? row)
}

class ReplicaController {
  private readonly views = new Map<string, ViewEntry>()
  private sink: RuntimeFrameHandlers | null = null
  private seq = 1_000_000
  private readonly now: () => number
  private readonly deps: ReplicaAdapterDeps

  constructor(deps: ReplicaAdapterDeps) {
    this.deps = deps
    this.now = deps.now ?? (() => Date.now())
  }

  async openMailListView(
    request: RuntimeSessionViewRequest,
  ): Promise<RuntimeOpenMessageListViewResult> {
    const result =
      await this.deps.base.openRuntimeSessionMessageListView(request)
    const handle = this.deps.makeHandle()
    handle.ingestJson(
      JSON.stringify(replicaRowsFromViewState(result.snapshot.data)),
    )
    // Rehydrate any unconfirmed intent (durable across reload) over the base.
    for (const record of await this.deps.outbox.all()) {
      handle.acceptJson(
        JSON.stringify({
          mutationId: record.clientMutationId,
          messageId: record.messageId,
          assertion: record.assertion,
        }),
      )
    }
    const membership = membershipMailbox(
      request.view.scope as Parameters<typeof membershipMailbox>[0],
    )
    const entry: ViewEntry = {
      handle,
      membershipMailbox: membership,
      lastSnapshot: result.snapshot,
      lastProjectionJson: '',
    }
    this.views.set(result.viewId, entry)
    const snapshot = this.projectSnapshot(entry)
    return { viewId: result.viewId, snapshot }
  }

  closeView(request: RuntimeSessionViewCloseRequest): Promise<OkResponse> {
    this.views.delete(request.viewId)
    return this.deps.base.closeRuntimeSessionView(request)
  }

  subscribe(
    request: RuntimeFrameSubscriptionRequest,
    handlers: RuntimeFrameHandlers,
  ): RuntimeUnsubscribe {
    this.sink = handlers
    const wrapped: RuntimeFrameHandlers = {
      ...handlers,
      onFrame: (frame) => this.onBaseFrame(frame, handlers),
    }
    const unsubscribe = this.deps.base.subscribeRuntimeFrames(request, wrapped)
    return () => {
      if (this.sink === handlers) {
        this.sink = null
      }
      unsubscribe()
    }
  }

  async runMutation(request: RuntimeRunMutationRequest) {
    const translated = await parseMessageMutation(request)
    if (!translated) {
      return this.deps.base.runRuntimeMutation(request)
    }
    const clientMutationId = request.clientMutationId
    for (const entry of this.views.values()) {
      entry.handle.acceptJson(
        JSON.stringify({
          mutationId: clientMutationId,
          messageId: translated.messageId,
          assertion: translated.assertion,
        }),
      )
    }
    await this.deps.outbox.put({
      clientMutationId,
      messageId: translated.messageId,
      assertion: translated.assertion,
      runtimeMutationId: null,
      acceptedAt: this.now(),
    })
    this.emitChangedViews()

    try {
      const receipt = await this.deps.base.runRuntimeMutation(request)
      if (receipt.runtimeMutationId) {
        await this.deps.outbox.linkRuntimeMutationId(
          clientMutationId,
          receipt.runtimeMutationId,
        )
      }
      return receipt
    } catch (error) {
      // Synchronous rejection: retire the optimism and surface the revert.
      await this.settleAll(clientMutationId, 'failed')
      throw error
    }
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
        entry.handle.ingestJson(
          JSON.stringify(replicaRowsFromViewState(frame.snapshot.data)),
        )
        entry.lastSnapshot = frame.snapshot
        const snapshot = this.projectSnapshot(entry)
        handlers.onFrame({ ...frame, snapshot })
        return
      }
      case 'viewDelta': {
        const entry = this.views.get(frame.viewId)
        if (!entry) {
          handlers.onFrame(frame)
          return
        }
        // Fold the runtime delta into the held served base, then re-ingest the
        // reconstructed base (replace_base keeps unconfirmed optimism). Emit the
        // folded result as a viewReplace; emitting deltas to the renderer is a
        // later step (L6 U3c).
        const rows = applyRuntimeDelta(
          entry.lastSnapshot.data.rows,
          frame.delta,
        )
        entry.lastSnapshot = {
          ...entry.lastSnapshot,
          revision: frame.revision,
          data: { ...entry.lastSnapshot.data, rows },
        }
        entry.handle.ingestJson(
          JSON.stringify(replicaRowsFromViewState(entry.lastSnapshot.data)),
        )
        const snapshot = this.projectSnapshot(entry)
        handlers.onFrame({
          type: 'viewReplace',
          sessionSeq: frame.sessionSeq,
          viewId: frame.viewId,
          revision: frame.revision,
          snapshot,
        })
        return
      }
      case 'mutationSettlement': {
        const verdict = settlementVerdict(frame.state.status)
        if (verdict) {
          void this.settleAll(frame.state.clientMutationId, verdict)
        }
        handlers.onFrame(frame)
        return
      }
      default:
        handlers.onFrame(frame)
    }
  }

  private async settleAll(
    clientMutationId: string,
    verdict: 'confirmed' | 'failed',
  ): Promise<void> {
    for (const entry of this.views.values()) {
      entry.handle.settle(clientMutationId, verdict)
    }
    await this.deps.outbox.remove(clientMutationId)
    this.emitChangedViews()
  }

  /** Emit a synthesized `viewReplace` for every view whose projection moved. */
  private emitChangedViews(): void {
    if (!this.sink) {
      return
    }
    for (const [viewId, entry] of this.views) {
      const json = entry.handle.projectJson(entry.membershipMailbox)
      if (json === entry.lastProjectionJson) {
        continue
      }
      const snapshot = this.snapshotFrom(entry, json)
      this.sink.onFrame({
        type: 'viewReplace',
        sessionSeq: this.seq++,
        viewId,
        revision: entry.lastSnapshot.revision,
        snapshot,
      })
    }
  }

  private projectSnapshot(
    entry: ViewEntry,
  ): RuntimeViewSnapshot<RuntimeMailListViewState> {
    const json = entry.handle.projectJson(entry.membershipMailbox)
    return this.snapshotFrom(entry, json)
  }

  private snapshotFrom(
    entry: ViewEntry,
    projectionJson: string,
  ): RuntimeViewSnapshot<RuntimeMailListViewState> {
    entry.lastProjectionJson = projectionJson
    const projections = JSON.parse(projectionJson) as unknown[]
    return {
      ...entry.lastSnapshot,
      data: applyOptimisticRows(entry.lastSnapshot.data, projections),
    }
  }
}

/**
 * Build a replica adapter over a base adapter. Every method not concerned with
 * the mail-list optimism delegates to the base unchanged.
 */
export function createReplicaAdapter(deps: ReplicaAdapterDeps): RuntimeAdapter {
  const controller = new ReplicaController(deps)
  return {
    ...deps.base,
    openRuntimeSessionMessageListView: (request) =>
      controller.openMailListView(request),
    closeRuntimeSessionView: (request) => controller.closeView(request),
    subscribeRuntimeFrames: (request, handlers) =>
      controller.subscribe(request, handlers),
    runRuntimeMutation: (request) => controller.runMutation(request),
  }
}
