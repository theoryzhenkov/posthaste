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
  RuntimeOpenMessageListViewResult,
  RuntimeRunMutationRequest,
  RuntimeSessionViewCloseRequest,
  RuntimeSessionViewExtendRequest,
  RuntimeSessionViewRequest,
  RuntimeUnsubscribe,
  RuntimeViewSnapshot,
} from '../types'
import type { OkResponse } from '../../api/types'
import type {
  EntityStoreHandle,
  EntityStoreHandleFactory,
  ReplicaAssertion,
  SettlementVerdict,
} from './handle'
import { settlementVerdict } from './mapping'
import {
  buildMailListPredicateContext,
  resolveMailListPredicate,
  type MailListPredicate,
} from '../mailListSelfMaintained'
import type { OutboxStore } from './outboxStore'
import { parseMessageMutation } from './wasmUtil'

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

export interface EntityStoreAdapterDeps {
  base: RuntimeAdapter
  makeHandle: EntityStoreHandleFactory
  outbox: OutboxStore
  /** React Query cache the adapter writes mailbox counts into. Defaults to the
   * app singleton. */
  queryClient?: QueryClient
  now?: () => number
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
  private readonly handle: EntityStoreHandle
  private readonly queryClient: QueryClient

  constructor(deps: EntityStoreAdapterDeps) {
    this.deps = deps
    this.handle = deps.makeHandle()
    this.queryClient = deps.queryClient ?? singletonQueryClient
    this.now = deps.now ?? (() => Date.now())
  }

  async openMailListView(
    request: RuntimeSessionViewRequest,
  ): Promise<RuntimeOpenMessageListViewResult> {
    // Option (i): the view opens non-delta-capable, so the runtime serves full
    // `viewSnapshot`/`viewReplace` and the store re-derives from `message.updated`.
    const result =
      await this.deps.base.openRuntimeSessionMessageListView(request)
    const { viewId, snapshot } = result
    const rows = snapshot.data.rows
    const predicate = resolveMailListPredicate(
      request.view.scope,
      request.view.sort,
      buildMailListPredicateContext(this.queryClient),
    )
    this.handle.registerViewJson(
      viewId,
      JSON.stringify({
        predicate,
        sortField: request.view.sort ?? 'date',
        sortDirection: request.view.sortDir ?? 'desc',
        watermark: watermarkFromSnapshot(snapshot, rows),
      }),
    )
    // P1: seed the rows' message bases + place the rows in one atomic batch.
    this.handle.ingestBatchJson(JSON.stringify(projectionBatchFromRows(rows)))
    this.handle.setViewRowsJson(
      viewId,
      JSON.stringify(rows.map(toStoreRow)),
      JSON.stringify(watermarkFromSnapshot(snapshot, rows)),
    )
    this.handle.drainDirtyJson()
    // Rehydrate unconfirmed intent (durable across reload) over the base.
    for (const record of await this.deps.outbox.all()) {
      this.handle.acceptMutationJson(
        JSON.stringify({
          mutationId: record.clientMutationId,
          messageId: record.messageId,
          assertion: record.assertion,
        }),
      )
    }
    const projected = this.projectView(viewId)
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
    request: RuntimeSessionViewExtendRequest,
  ): Promise<RuntimeOpenMessageListViewResult> {
    const result = await this.deps.base.extendRuntimeSessionView(request)
    const entry = this.views.get(result.viewId)
    if (!entry) {
      // Untracked view (not opened through the store): pass through unchanged.
      return result
    }
    const rows = result.snapshot.data.rows
    this.handle.ingestBatchJson(JSON.stringify(projectionBatchFromRows(rows)))
    this.handle.setViewRowsJson(
      result.viewId,
      JSON.stringify(rows.map(toStoreRow)),
      JSON.stringify(watermarkFromSnapshot(result.snapshot, rows)),
    )
    entry.lastSnapshot = result.snapshot
    this.handle.drainDirtyJson()
    entry.lastProjectionJson = this.projectView(result.viewId).json
    return result
  }

  closeView(request: RuntimeSessionViewCloseRequest): Promise<OkResponse> {
    this.handle.closeView(request.viewId)
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
    const translated = await parseMessageMutation(
      request,
      this.roleMapForRequest(request),
    )
    if (!translated) {
      return this.deps.base.runRuntimeMutation(request)
    }
    const clientMutationId = request.clientMutationId
    this.handle.acceptMutationJson(
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
    })
    this.drainAndEmit()

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
        // A served snapshot / page / resync: re-seed the rows' bases + place
        // them (P1), then re-project from the store.
        const rows = frame.snapshot.data.rows
        this.handle.ingestBatchJson(
          JSON.stringify(projectionBatchFromRows(rows)),
        )
        this.handle.setViewRowsJson(
          frame.viewId,
          JSON.stringify(rows.map(toStoreRow)),
          JSON.stringify(watermarkFromSnapshot(frame.snapshot, rows)),
        )
        entry.lastSnapshot = frame.snapshot
        this.drainAndEmit()
        void this.clearRetired()
        return
      }
      case 'notification': {
        const event = frame.payload as DomainEvent | undefined
        if (event?.topic === 'message.updated') {
          this.ingestMessageEvent(event)
          this.drainAndEmit()
          void this.clearRetired()
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
  private ingestMessageEvent(event: DomainEvent): void {
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
      return
    }
    const deleted = inner?.deleted === true
    const projection = inner?.projection ?? null
    const countDeltas = inner?.countDeltas ?? []
    // Not deleted + no projection: nothing to materialize (shouldn't happen —
    // 2c attaches the projection to every non-destroy event).
    if (!deleted && !projection) {
      return
    }
    const batch: StoreUpdate[] = [
      {
        message: { messageId, projection, deleted, countDeltas },
      },
    ]
    this.handle.ingestBatchJson(JSON.stringify(batch))
    const accountId = event.accountId
    if (accountId) {
      for (const delta of countDeltas) {
        this.mailboxAccount.set(delta.mailboxId, accountId)
      }
    }
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
    const sourceId = (
      request.args as { sourceId?: string } | undefined
    )?.sourceId
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
    this.handle.settle(clientMutationId, verdict)
    await this.clearRetired()
    this.drainAndEmit()
  }

  /** Clear durable-outbox records for ops the engine retired since the last
   *  drain (settle-confirm or base catch-up). An un-retired op stays durable so
   *  it survives a reload to be replayed. (outbox D) */
  private async clearRetired(): Promise<void> {
    const retired = JSON.parse(this.handle.drainRetiredJson()) as string[]
    if (retired.length) {
      await Promise.all(retired.map((id) => this.deps.outbox.remove(id)))
    }
  }

  /** Drain the store's dirty keys, re-project open views, and write counts. */
  private drainAndEmit(): void {
    const dirty = JSON.parse(this.handle.drainDirtyJson()) as DirtyKey[]
    // Re-project ALL open views (D-2e-2: covers content-only mutations — P2 —
    // without a message→rows index).
    this.emitChangedViews()
    for (const key of dirty) {
      if ('mailbox' in key) {
        this.writeMailboxCount(key.mailbox)
      }
    }
  }

  /** Emit a synthesized `viewReplace` for every view whose projection moved. */
  private emitChangedViews(): void {
    if (!this.sink) {
      return
    }
    for (const [viewId, entry] of this.views) {
      const projected = this.projectView(viewId)
      if (projected.json === entry.lastProjectionJson) {
        continue
      }
      entry.lastProjectionJson = projected.json
      const snapshot = this.snapshotFrom(entry, projected.rows)
      this.sink.onFrame({
        type: 'viewReplace',
        sessionSeq: this.seq++,
        viewId,
        revision: entry.lastSnapshot.revision,
        snapshot,
      })
    }
  }

  /** Re-project a view from the store: the joined rows → renderer rows. */
  private projectView(viewId: string): {
    json: string
    rows: RuntimeMailListRowState[]
  } {
    const json = this.handle.projectViewJson(viewId)
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
  private writeMailboxCount(mailboxId: string): void {
    const accountId = this.mailboxAccount.get(mailboxId)
    if (!accountId) {
      return
    }
    const counts = JSON.parse(this.handle.mailboxJson(mailboxId)) as {
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
    openRuntimeSessionMessageListView: (request) =>
      controller.openMailListView(request),
    extendRuntimeSessionView: (request) =>
      controller.extendMailListView(request),
    closeRuntimeSessionView: (request) => controller.closeView(request),
    subscribeRuntimeFrames: (request, handlers) =>
      controller.subscribe(request, handlers),
    runRuntimeMutation: (request) => controller.runMutation(request),
  }
}
