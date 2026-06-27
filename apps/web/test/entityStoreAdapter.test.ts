import { describe, expect, it } from 'bun:test'

import { QueryClient } from '@tanstack/react-query'

import type { OkResponse } from '../src/api/types'
import type { Mailbox } from '../src/api/types'
import { queryKeys } from '../src/queryKeys'
import { createEntityStoreAdapter } from '../src/runtime/replica/entityStoreAdapter'
import type {
  EntityStoreHandle,
  ReplicaAssertion,
  SettlementVerdict,
} from '../src/runtime/replica/handle'
import { MemoryOutboxStore } from '../src/runtime/replica/outboxStore'
import type {
  RuntimeAdapter,
  RuntimeFrame,
  RuntimeFrameHandlers,
  RuntimeMailListViewState,
  RuntimeMutationReceipt,
  RuntimeRunMutationRequest,
  RuntimeSessionViewRequest,
  RuntimeViewSnapshot,
} from '../src/runtime/types'

// --- A faithful in-TS entity-store handle (the WASM handle is covered by the
// Rust tests + the smoke test); this exercises the controller's orchestration.
// Mirrors the store: per-key message bases + the optimism fold, evaluable-view
// self-maintenance, authority-only mailbox counts, dirty tracking. ---

interface StoreViewRow {
  rowKey: string
  messageId: string
  sortKey: { receivedAt: string; messageId: string }
}
type ViewPredicate = { inMailbox: string } | 'all' | 'deferred'
type DirtyKey = { message: string } | { mailbox: string } | { view: string }

function dirtyKey(key: DirtyKey): string {
  if ('message' in key) return `message:${key.message}`
  if ('mailbox' in key) return `mailbox:${key.mailbox}`
  return `view:${key.view}`
}

class FakeHandle implements EntityStoreHandle {
  private readonly messages = new Map<string, Record<string, unknown>>()
  private readonly mailboxes = new Map<
    string,
    { unreadCount: number; totalCount: number }
  >()
  private readonly views = new Map<
    string,
    { predicate: ViewPredicate; rows: StoreViewRow[]; watermark: unknown }
  >()
  private readonly pending = new Map<
    string,
    { messageId: string; assertion: ReplicaAssertion }
  >()
  private readonly dirty = new Set<string>()

  registerViewJson(viewId: string, argsJson: string): void {
    const args = JSON.parse(argsJson) as {
      predicate: ViewPredicate
      watermark: unknown
    }
    this.views.set(viewId, {
      predicate: args.predicate,
      rows: [],
      watermark: null,
    })
    this.dirty.add(dirtyKey({ view: viewId }))
  }

  setViewRowsJson(
    viewId: string,
    rowsJson: string,
    watermarkJson: string,
  ): void {
    const rows = JSON.parse(rowsJson) as StoreViewRow[]
    const view = this.views.get(viewId)
    if (view) {
      view.rows = rows
      view.watermark = JSON.parse(watermarkJson)
    }
    this.dirty.add(dirtyKey({ view: viewId }))
  }

  closeView(viewId: string): void {
    this.views.delete(viewId)
  }

  ingestBatchJson(batchJson: string): void {
    const batch = JSON.parse(batchJson) as Array<
      | {
          message: {
            messageId: string
            projection: Record<string, unknown>
            deleted: boolean
            countDeltas: Array<{
              mailboxId: string
              unreadCount: number
              totalCount: number
            }>
          }
        }
      | {
          mailboxCount: {
            mailboxId: string
            unreadCount: number
            totalCount: number
          }
        }
    >
    for (const update of batch) {
      if ('message' in update) {
        const { messageId, projection, deleted, countDeltas } = update.message
        if (deleted) {
          this.messages.delete(messageId)
          this.pending.forEach((op, id) => {
            if (op.messageId === messageId) this.pending.delete(id)
          })
          this.removeMessageFromViews(messageId)
        } else {
          this.messages.set(messageId, projection)
          this.rederive(messageId)
        }
        this.dirty.add(dirtyKey({ message: messageId }))
        for (const delta of countDeltas) {
          this.mailboxes.set(delta.mailboxId, {
            unreadCount: delta.unreadCount,
            totalCount: delta.totalCount,
          })
          this.dirty.add(dirtyKey({ mailbox: delta.mailboxId }))
        }
      } else {
        this.mailboxes.set(update.mailboxCount.mailboxId, {
          unreadCount: update.mailboxCount.unreadCount,
          totalCount: update.mailboxCount.totalCount,
        })
        this.dirty.add(dirtyKey({ mailbox: update.mailboxCount.mailboxId }))
      }
    }
  }

  acceptMutationJson(acceptJson: string): void {
    const { mutationId, messageId, assertion } = JSON.parse(acceptJson) as {
      mutationId: string
      messageId: string
      assertion: ReplicaAssertion
    }
    this.pending.set(mutationId, { messageId, assertion })
    if (this.messages.has(messageId)) {
      this.rederive(messageId)
    }
    this.dirty.add(dirtyKey({ message: messageId }))
  }

  settle(mutationId: string, _outcome: SettlementVerdict): boolean {
    const op = this.pending.get(mutationId)
    if (!op) {
      return false
    }
    this.pending.delete(mutationId)
    if (this.messages.has(op.messageId)) {
      this.rederive(op.messageId)
    }
    return _outcome === 'failed' // a failure reverts the fold
  }

  hasPending(): boolean {
    return this.pending.size > 0
  }

  messageJson(messageId: string): string {
    return JSON.stringify(this.foldedProjection(messageId) ?? null)
  }

  mailboxJson(mailboxId: string): string {
    return JSON.stringify(this.mailboxes.get(mailboxId) ?? null)
  }

  viewRowsJson(viewId: string): string {
    const view = this.views.get(viewId)
    return JSON.stringify(view?.rows ?? null)
  }

  projectViewJson(viewId: string): string {
    const view = this.views.get(viewId)
    if (!view) {
      return 'null'
    }
    const projected = view.rows.map((row) => ({
      rowKey: row.rowKey,
      messageId: row.messageId,
      sortKey: row.sortKey,
      projection: this.foldedProjection(row.messageId) ?? null,
    }))
    return JSON.stringify(projected)
  }

  drainDirtyJson(): string {
    const keys = [...this.dirty].map((encoded) => {
      const [kind, id] = encoded.split(':')
      return kind === 'message'
        ? { message: id }
        : kind === 'mailbox'
          ? { mailbox: id }
          : { view: id }
    })
    this.dirty.clear()
    return JSON.stringify(keys)
  }

  /** Fold the pending outbox over a message's base projection (optimism). */
  private foldedProjection(
    messageId: string,
  ): Record<string, unknown> | undefined {
    const base = this.messages.get(messageId)
    if (!base) {
      return undefined
    }
    let projection = { ...base }
    let destroyed = false
    for (const op of this.pending.values()) {
      if (op.messageId !== messageId) {
        continue
      }
      if (op.assertion.kind === 'setKeywords') {
        const keywords = new Set(
          (projection.keywords as string[] | undefined) ?? [],
        )
        for (const k of op.assertion.remove) {
          keywords.delete(k)
        }
        for (const k of op.assertion.add) {
          keywords.add(k)
        }
        projection = { ...projection, keywords: [...keywords] }
      } else if (op.assertion.kind === 'replaceMailboxes') {
        projection = { ...projection, mailboxIds: [...op.assertion.mailboxIds] }
      } else if (op.assertion.kind === 'destroy') {
        destroyed = true
      }
    }
    return destroyed ? undefined : projection
  }

  /** Re-evaluate a held message's placement across evaluable views. */
  private rederive(messageId: string): void {
    const projection = this.foldedProjection(messageId)
    for (const [viewId, view] of this.views) {
      if (view.predicate === 'deferred') {
        continue
      }
      const matches =
        projection != null &&
        (view.predicate === 'all' ||
          (view.predicate.inMailbox &&
            ((projection.mailboxIds as string[] | undefined) ?? []).includes(
              view.predicate.inMailbox,
            )))
      const index = view.rows.findIndex((r) => r.messageId === messageId)
      if (matches && projection) {
        const receivedAt = (projection.receivedAt as string | undefined) ?? ''
        const row: StoreViewRow = {
          rowKey: `${projection.sourceId}:${messageId}`,
          messageId,
          sortKey: { receivedAt, messageId },
        }
        if (index >= 0) {
          view.rows[index] = row
        } else {
          view.rows.push(row)
        }
      } else if (index >= 0) {
        view.rows.splice(index, 1)
      }
      this.dirty.add(dirtyKey({ view: viewId }))
    }
  }

  private removeMessageFromViews(messageId: string): void {
    for (const [viewId, view] of this.views) {
      const index = view.rows.findIndex((r) => r.messageId === messageId)
      if (index >= 0) {
        view.rows.splice(index, 1)
        this.dirty.add(dirtyKey({ view: viewId }))
      }
    }
  }
}

// --- A fake base adapter exposing the overridden surfaces + a push. ---

interface BaseRow {
  messageId: string
  receivedAt: string
  keywords: string[]
  mailboxIds: string[]
}

function snapshot(
  rows: BaseRow[],
): RuntimeViewSnapshot<RuntimeMailListViewState> {
  return {
    viewId: 'v1',
    descriptor: { family: 'mailList', payload: {} },
    revision: 1,
    lifecycle: 'ready',
    readWatermark: null,
    coverage: { ranges: [] },
    data: {
      scope: null,
      projectionKind: 'message',
      sort: null,
      windowRequest: null,
      rows: rows.map((row) => ({
        rowKey: `s:${row.messageId}`,
        resourceRef: `message:s:${row.messageId}`,
        projection: {
          id: row.messageId,
          sourceId: 's',
          receivedAt: row.receivedAt,
          keywords: row.keywords,
          mailboxIds: row.mailboxIds,
          isRead: row.keywords.includes('$seen'),
          isFlagged: row.keywords.includes('$flagged'),
          subject: row.messageId,
        } as never,
        orderKey: row.messageId,
      })),
      continuation: {
        beforeCursor: null,
        afterCursor: null,
        hasBefore: false,
        hasAfter: false,
      },
      readWatermark: null,
      coverage: { ranges: [] },
      knownTotalCount: rows.length,
      pendingMutations: [],
      anchor: null,
    },
    pendingMutations: [],
    error: null,
  }
}

function makeBase(rows: BaseRow[]) {
  let frameSink: RuntimeFrameHandlers | null = null
  const mutations: RuntimeRunMutationRequest[] = []
  const receipt: RuntimeMutationReceipt = {
    runtimeMutationId: 'r-1',
    clientMutationId: 'c-1',
    name: 'message.setKeywords',
    state: 'accepted',
    error: null,
  }
  const base = {
    openRuntimeSessionMessageListView: async () => ({
      viewId: 'v1',
      snapshot: snapshot(rows),
    }),
    closeRuntimeSessionView: async (): Promise<OkResponse> => ({ ok: true }),
    subscribeRuntimeFrames: (
      _request: unknown,
      handlers: RuntimeFrameHandlers,
    ) => {
      frameSink = handlers
      return () => {
        frameSink = null
      }
    },
    runRuntimeMutation: async (request: RuntimeRunMutationRequest) => {
      mutations.push(request)
      return { ...receipt, clientMutationId: request.clientMutationId }
    },
  } as unknown as RuntimeAdapter
  return {
    base,
    mutations,
    push: (frame: RuntimeFrame<RuntimeMailListViewState>) =>
      frameSink?.onFrame(frame),
  }
}

const viewRequest: RuntimeSessionViewRequest = {
  sessionId: 'sess',
  view: {
    scope: { kind: 'source-mailbox', sourceId: 's', mailboxId: 'inbox' },
    limit: 50,
    sort: 'date',
    sortDir: 'desc',
    operation: { name: 'test' } as never,
  },
}

function row(
  id: string,
  receivedAt: string,
  keywords: string[] = [],
  mailboxes: string[] = ['inbox'],
): BaseRow {
  return { messageId: id, receivedAt, keywords, mailboxIds: mailboxes }
}

function setFlagged(
  id: string,
  clientMutationId: string,
): RuntimeRunMutationRequest {
  return {
    sessionId: 'sess',
    name: 'message.setKeywords',
    args: {
      sourceId: 's',
      messageId: id,
      command: { add: ['$flagged'], remove: [] },
    },
    clientMutationId,
  }
}

function keywordsOf(
  frames: RuntimeFrame<RuntimeMailListViewState>[],
  messageId: string,
): string[] | undefined {
  const last = [...frames]
    .reverse()
    .find((f) => f.type === 'viewReplace' || f.type === 'viewSnapshot')
  if (last?.type !== 'viewReplace' && last?.type !== 'viewSnapshot') {
    return undefined
  }
  const projection = last.snapshot.data.rows.find(
    (r) => (r.projection as { id: string }).id === messageId,
  )?.projection as { keywords?: string[] } | undefined
  return projection?.keywords
}

function messageUpdated(
  messageId: string,
  projection: Record<string, unknown>,
  countDeltas: Array<{
    mailboxId: string
    unreadCount: number
    totalCount: number
  }> = [],
  accountId = 's',
): RuntimeFrame<RuntimeMailListViewState> {
  return {
    type: 'notification',
    sessionSeq: 100,
    kind: 'message.updated',
    payload: {
      seq: 1,
      accountId,
      topic: 'message.updated',
      occurredAt: 'now',
      payload: { messageId, projection, countDeltas },
    },
  }
}

function build() {
  const harness = makeBase([
    row('m1', '2026-04-29T10:00:00Z'),
    row('m2', '2026-04-28T10:00:00Z'),
  ])
  const outbox = new MemoryOutboxStore()
  const queryClient = new QueryClient()
  // Seed the sidebar's mailbox cache so count writes have a row to update.
  queryClient.setQueryData<Mailbox[]>(queryKeys.mailboxes('s'), [
    {
      id: 'inbox',
      name: 'Inbox',
      role: 'inbox',
      unreadEmails: 2,
      totalEmails: 2,
    },
  ])
  const adapter = createEntityStoreAdapter({
    base: harness.base,
    makeHandle: () => new FakeHandle(),
    outbox,
    queryClient,
    now: () => 1,
  })
  const frames: RuntimeFrame<RuntimeMailListViewState>[] = []
  adapter.subscribeRuntimeFrames(
    { sessionId: 'sess' },
    { onFrame: (f) => frames.push(f) },
  )
  return { adapter, outbox, frames, harness, queryClient }
}

describe('entityStoreAdapter', () => {
  it('returns the served base as the initial projected snapshot', async () => {
    const { adapter } = build()
    const opened = await adapter.openRuntimeSessionMessageListView(viewRequest)
    expect(
      opened.snapshot.data.rows.map((r) => (r.projection as { id: string }).id),
    ).toEqual(['m1', 'm2'])
  })

  it('folds a message mutation optimistically + forwards the POST + outbox', async () => {
    const { adapter, outbox, frames, harness } = build()
    await adapter.openRuntimeSessionMessageListView(viewRequest)

    const receipt = await adapter.runRuntimeMutation(setFlagged('m1', 'c1'))

    expect(keywordsOf(frames, 'm1')).toContain('$flagged')
    expect(harness.mutations.map((m) => m.clientMutationId)).toEqual(['c1'])
    const records = await outbox.all()
    expect(records).toHaveLength(1)
    expect(records[0]?.runtimeMutationId).toBe('r-1')
    expect(receipt.clientMutationId).toBe('c1')
  })

  it('reverts optimism + clears the outbox on failure', async () => {
    const built = build()
    const { adapter, outbox, frames } = built
    await adapter.openRuntimeSessionMessageListView(viewRequest)
    await adapter.runRuntimeMutation(setFlagged('m1', 'c1'))
    expect(keywordsOf(frames, 'm1')).toContain('$flagged')

    built.harness.push({
      type: 'mutationSettlement',
      sessionSeq: 5,
      mutationId: 'r-1',
      state: {
        clientMutationId: 'c1',
        name: 'message.setKeywords',
        status: 'failed',
        error: null,
      },
    })
    await Promise.resolve()
    await Promise.resolve()

    expect(await outbox.all()).toHaveLength(0)
    expect(keywordsOf(frames, 'm1')).not.toContain('$flagged')
  })

  it('ingests a message.updated notification and re-projects the row', async () => {
    const built = build()
    const { adapter, frames } = built
    await adapter.openRuntimeSessionMessageListView(viewRequest)

    // An authoritative message.updated: m1 gains $flagged + the inbox count
    // moves. The store ingests it (one batch) + the adapter re-projects.
    built.harness.push(
      messageUpdated(
        'm1',
        {
          id: 'm1',
          sourceId: 's',
          receivedAt: '2026-04-29T10:00:00Z',
          keywords: ['$flagged'],
          mailboxIds: ['inbox'],
          isRead: false,
          isFlagged: true,
          subject: 'm1',
        },
        [{ mailboxId: 'inbox', unreadCount: 2, totalCount: 2 }],
      ),
    )
    await Promise.resolve()

    expect(keywordsOf(frames, 'm1')).toContain('$flagged')
  })

  it('writes the count delta straight into the React Query cache (no refetch)', async () => {
    const built = build()
    const { adapter, queryClient } = built
    await adapter.openRuntimeSessionMessageListView(viewRequest)

    built.harness.push(
      messageUpdated(
        'm1',
        {
          id: 'm1',
          sourceId: 's',
          receivedAt: '2026-04-29T10:00:00Z',
          keywords: ['$seen'],
          mailboxIds: ['inbox'],
          isRead: true,
          isFlagged: false,
          subject: 'm1',
        },
        [{ mailboxId: 'inbox', unreadCount: 1, totalCount: 2 }],
      ),
    )
    await Promise.resolve()

    const mailboxes = queryClient.getQueryData<Mailbox[]>(
      queryKeys.mailboxes('s'),
    )
    expect(mailboxes?.find((m) => m.id === 'inbox')?.unreadEmails).toBe(1)
  })

  it('passes non-message mutations straight through', async () => {
    const { adapter, outbox, harness } = build()
    await adapter.openRuntimeSessionMessageListView(viewRequest)
    await adapter.runRuntimeMutation({
      sessionId: 'sess',
      name: 'account.sync',
      args: { sourceId: 's' },
      clientMutationId: 'c9',
    })
    expect(harness.mutations.map((m) => m.name)).toEqual(['account.sync'])
    expect(await outbox.all()).toHaveLength(0)
  })

  it('forwards unrelated frames unchanged (parity)', async () => {
    const built = build()
    const { adapter, frames } = built
    await adapter.openRuntimeSessionMessageListView(viewRequest)
    built.harness.push({ type: 'heartbeat', sessionSeq: 3 })
    expect(frames.at(-1)).toEqual({ type: 'heartbeat', sessionSeq: 3 })
  })
})
