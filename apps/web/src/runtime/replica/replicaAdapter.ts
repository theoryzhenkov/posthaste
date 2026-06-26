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
import type { ReplicaHandleFactory, RuntimeReplicaHandle } from './handle'
import { membershipMailbox, settlementVerdict } from './mapping'
import type { OutboxStore } from './outboxStore'
import { parseMessageMutation } from './wasmUtil'

export interface ReplicaAdapterDeps {
  base: RuntimeAdapter
  makeHandle: ReplicaHandleFactory
  outbox: OutboxStore
  now?: () => number
}

interface ViewEntry {
  handle: RuntimeReplicaHandle
  membershipMailbox: string | null
  lastSnapshot: RuntimeViewSnapshot<RuntimeMailListViewState>
  lastProjectionJson: string
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
    handle.ingestViewJson(JSON.stringify(result.snapshot.data.rows))
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
    const snapshot = this.projectView(entry)
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
        entry.handle.ingestViewJson(JSON.stringify(frame.snapshot.data.rows))
        entry.lastSnapshot = frame.snapshot
        const snapshot = this.projectView(entry)
        handlers.onFrame({ ...frame, snapshot })
        return
      }
      case 'viewDelta': {
        const entry = this.views.get(frame.viewId)
        if (!entry) {
          handlers.onFrame(frame)
          return
        }
        // Delegate delta reconciliation to the WASM replica (which keeps
        // pending optimism) and emit the folded result as a viewReplace.
        entry.handle.applyDeltaJson(JSON.stringify(frame.delta))
        entry.lastSnapshot = {
          ...entry.lastSnapshot,
          revision: frame.revision,
        }
        const snapshot = this.projectView(entry)
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
      const json = entry.handle.projectViewJson(entry.membershipMailbox)
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

  private projectView(
    entry: ViewEntry,
  ): RuntimeViewSnapshot<RuntimeMailListViewState> {
    const json = entry.handle.projectViewJson(entry.membershipMailbox)
    return this.snapshotFrom(entry, json)
  }

  private snapshotFrom(
    entry: ViewEntry,
    rowsJson: string,
  ): RuntimeViewSnapshot<RuntimeMailListViewState> {
    entry.lastProjectionJson = rowsJson
    const rows = JSON.parse(rowsJson) as RuntimeMailListRowState[]
    return {
      ...entry.lastSnapshot,
      data: { ...entry.lastSnapshot.data, rows },
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
