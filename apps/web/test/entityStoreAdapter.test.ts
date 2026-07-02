import { beforeAll, describe, expect, it } from 'bun:test'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

import { QueryClient } from '@tanstack/react-query'

import type { OkResponse } from '../src/api/types'
import type { Mailbox } from '../src/api/types'
import { queryKeys } from '../src/queryKeys'
import { createEntityStoreAdapter } from '../src/runtime/replica/entityStoreAdapter'
import type { EntityStoreHandle } from '../src/runtime/replica/handle'
import { MemoryOutboxStore } from '../src/runtime/replica/outboxStore'
import {
  MemoryUndoHistoryStore,
  resetUndoHistoryStoreForTesting,
  setUndoHistoryStoreForTesting,
} from '../src/runtime/replica/undoHistoryStore'
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
import type { NearEndOutboxHooks } from '../src/runtime/nearEnd'

// The adapter test drives the REAL wasm EntityStore handle (not a TS re-impl of
// the engine), so the controller's orchestration is verified against the engine
// that ships. The wasm bundle is a committed artifact; load + initialize it once.
const wasmDir = join(import.meta.dir, '..', 'src', 'runtime', 'wasm')
let makeRealHandle: () => EntityStoreHandle

beforeAll(async () => {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const mod = (await import(join(wasmDir, 'posthaste_link_wasm.js'))) as any
  // Bun: initialize synchronously from the binary (avoids the file:// fetch).
  mod.initSync({
    module: readFileSync(join(wasmDir, 'posthaste_link_wasm_bg.wasm')),
  })
  makeRealHandle = () => new mod.EntityStoreHandle() as EntityStoreHandle
})

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
    extendRuntimeSessionView: async () => ({
      viewId: 'v1',
      snapshot: snapshot([
        ...rows,
        row('m3', '2026-04-27T10:00:00Z'),
        row('m4', '2026-04-26T10:00:00Z'),
      ]),
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
  // The adapter registers its durable-outbox hooks with the near-end engine at
  // construction (D44); the stub captures them so tests can drive the
  // reconciler's calls directly (the engine's TIMING is pinned by the
  // `nearEndEngine` suite over fake IO).
  const captured: { hooks: NearEndOutboxHooks | null } = { hooks: null }
  const adapter = createEntityStoreAdapter({
    base: harness.base,
    makeHandle: () => makeRealHandle(),
    outbox,
    queryClient,
    now: () => 1,
    nearEnd: {
      setOutboxHooks: (hooks) => {
        captured.hooks = hooks
      },
      sessionId: () => 'sess-live',
    },
  })
  const hooks = () => {
    if (!captured.hooks) throw new Error('outbox hooks were not registered')
    return captured.hooks
  }
  const frames: RuntimeFrame<RuntimeMailListViewState>[] = []
  adapter.subscribeRuntimeFrames(
    { sessionId: 'sess' },
    { onFrame: (f) => frames.push(f) },
  )
  return { adapter, outbox, frames, harness, queryClient, hooks }
}

/** Drain all pending microtasks (the serialized store queue) via a macrotask. */
const tick = () => new Promise<void>((resolve) => setTimeout(resolve, 0))

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
      type: 'mutationNotification',
      sessionSeq: 5,
      clientMutationId: 'c1',
      notification: {
        type: 'rejected',
        error: {
          code: 'conflict',
          message: 'rejected by the authority',
          retryable: false,
        },
      },
    })
    await tick()

    expect(await outbox.all()).toHaveLength(0)
    expect(keywordsOf(frames, 'm1')).not.toContain('$flagged')
  })

  it('confirmed before the base update does not revert (flicker fix)', async () => {
    const built = build()
    const { adapter, outbox, frames } = built
    await adapter.openRuntimeSessionMessageListView(viewRequest)
    await adapter.runRuntimeMutation(setFlagged('m1', 'c1'))
    expect(keywordsOf(frames, 'm1')).toContain('$flagged')

    // The confirmation outruns the authoritative message.updated. It must NOT
    // flip the row back to the un-flagged base; the durable outbox RETAINS the
    // op (outbox D: not retired yet — the base hasn't caught up to absorb it).
    built.harness.push({
      type: 'mutationNotification',
      sessionSeq: 5,
      clientMutationId: 'c1',
      notification: { type: 'confirmed' },
    })
    await tick()
    expect(keywordsOf(frames, 'm1')).toContain('$flagged')
    expect(await outbox.all()).toHaveLength(1)

    // The base then catches up; still flagged, no revert anywhere.
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
        [],
      ),
    )
    await tick()
    expect(keywordsOf(frames, 'm1')).toContain('$flagged')
    expect(await outbox.all()).toHaveLength(0)
  })

  it('rehydration keeps reconcilable sent records and drops legacy ones', async () => {
    const { adapter, outbox, hooks } = build()
    // A prior session left durable records: a sent one WITH its dispatch
    // session (the engine's reconciler can query its settlement — D44b, keep),
    // a legacy sent one WITHOUT (unqueryable → dropped, the old leak guard),
    // and a never-sent one (still-unconfirmed intent that must survive).
    await outbox.put({
      clientMutationId: 'sent-reconcilable',
      messageId: 'm1',
      assertion: { kind: 'setKeywords', add: ['$flagged'], remove: [] },
      runtimeMutationId: 'r-prev',
      acceptedAt: 1,
      sessionId: 'sess-old',
      request: setFlagged('m1', 'sent-reconcilable'),
    })
    await outbox.put({
      clientMutationId: 'sent-legacy',
      messageId: 'm1',
      assertion: { kind: 'setKeywords', add: ['$flagged'], remove: [] },
      runtimeMutationId: 'r-prev-2',
      acceptedAt: 1,
    })
    await outbox.put({
      clientMutationId: 'never-sent-1',
      messageId: 'm2',
      assertion: { kind: 'setKeywords', add: ['$flagged'], remove: [] },
      runtimeMutationId: null,
      acceptedAt: 1,
    })

    await adapter.openRuntimeSessionMessageListView(viewRequest)

    const remaining = await outbox.all()
    expect(remaining.map((r) => r.clientMutationId).sort()).toEqual([
      'never-sent-1',
      'sent-reconcilable',
    ])
    // The reconcilable record is exactly what the engine's sent-but-unsettled
    // query sees, keyed to its ORIGINAL session.
    const unsettled = await hooks().sentUnsettled()
    expect(unsettled).toEqual([
      {
        sessionId: 'sess-old',
        clientMutationId: 'sent-reconcilable',
        request: setFlagged('m1', 'sent-reconcilable'),
      },
    ])
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
    await tick()

    expect(keywordsOf(frames, 'm1')).toContain('$flagged')
  })

  it('coalesces a message.updated burst into one re-projection per flush', async () => {
    // Manual scheduler: capture the flush instead of running it, so we can drive
    // a burst and assert it is applied as a single batch (the sync-burst fix).
    let scheduledFlush: (() => void) | null = null
    const harness = makeBase([
      row('m1', '2026-04-29T10:00:00Z'),
      row('m2', '2026-04-28T10:00:00Z'),
    ])
    const queryClient = new QueryClient()
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
      makeHandle: () => makeRealHandle(),
      outbox: new MemoryOutboxStore(),
      queryClient,
      now: () => 1,
      scheduleFlush: (cb) => {
        scheduledFlush = cb
        return () => {
          scheduledFlush = null
        }
      },
    })
    const frames: RuntimeFrame<RuntimeMailListViewState>[] = []
    adapter.subscribeRuntimeFrames(
      { sessionId: 'sess' },
      { onFrame: (f) => frames.push(f) },
    )
    await adapter.openRuntimeSessionMessageListView(viewRequest)
    const viewReplacesBefore = frames.filter(
      (f) => f.type === 'viewReplace',
    ).length

    // A burst: m1 + m2 both gain $flagged.
    const flagged = (id: string, receivedAt: string) =>
      messageUpdated(id, {
        id,
        sourceId: 's',
        receivedAt,
        keywords: ['$flagged'],
        mailboxIds: ['inbox'],
        isRead: false,
        isFlagged: true,
        subject: id,
      })
    harness.push(flagged('m1', '2026-04-29T10:00:00Z'))
    harness.push(flagged('m2', '2026-04-28T10:00:00Z'))

    // Buffered: nothing re-projected yet, and exactly one flush is scheduled.
    expect(frames.filter((f) => f.type === 'viewReplace').length).toBe(
      viewReplacesBefore,
    )
    expect(scheduledFlush).not.toBeNull()

    // One flush projects the whole batch → a SINGLE viewReplace with both rows.
    scheduledFlush!()
    await tick()
    expect(frames.filter((f) => f.type === 'viewReplace').length).toBe(
      viewReplacesBefore + 1,
    )
    expect(keywordsOf(frames, 'm1')).toContain('$flagged')
    expect(keywordsOf(frames, 'm2')).toContain('$flagged')
  })

  it('folds a multi-frame flush into a SINGLE store ingest (P3 batching)', async () => {
    // Count ingestBatchJson on the real handle: a coalesced flush must apply the
    // whole burst in one ingest, not one per frame (the round-trip that made the
    // worker drain 16x slower before P3).
    let ingestCount = 0
    const real = makeRealHandle()
    const countingHandle = new Proxy(real, {
      get(target, prop, receiver) {
        const value = Reflect.get(target, prop, receiver)
        if (typeof value !== 'function') return value
        if (prop === 'ingestBatchJson') {
          return (...args: unknown[]) => {
            ingestCount += 1
            return (value as (...a: unknown[]) => unknown).apply(target, args)
          }
        }
        return (value as (...a: unknown[]) => unknown).bind(target)
      },
    }) as EntityStoreHandle

    let scheduledFlush: (() => void) | null = null
    const harness = makeBase([
      row('m1', '2026-04-29T10:00:00Z'),
      row('m2', '2026-04-28T10:00:00Z'),
    ])
    const adapter = createEntityStoreAdapter({
      base: harness.base,
      makeHandle: () => countingHandle,
      outbox: new MemoryOutboxStore(),
      now: () => 1,
      scheduleFlush: (cb) => {
        scheduledFlush = cb
        return () => {
          scheduledFlush = null
        }
      },
    })
    adapter.subscribeRuntimeFrames({ sessionId: 'sess' }, { onFrame: () => {} })
    await adapter.openRuntimeSessionMessageListView(viewRequest)

    const ingestsAfterOpen = ingestCount
    const flagged = (id: string, receivedAt: string) =>
      messageUpdated(id, {
        id,
        sourceId: 's',
        receivedAt,
        keywords: ['$flagged'],
        mailboxIds: ['inbox'],
        isRead: false,
        isFlagged: true,
        subject: id,
      })
    harness.push(flagged('m1', '2026-04-29T10:00:00Z'))
    harness.push(flagged('m2', '2026-04-28T10:00:00Z'))
    scheduledFlush!()
    await tick()

    // Two frames in the flush → exactly ONE ingest, not two.
    expect(ingestCount - ingestsAfterOpen).toBe(1)
  })

  it('extend seeds the store so a later message.updated keeps the extended rows', async () => {
    const built = build()
    const { adapter, frames } = built
    await adapter.openRuntimeSessionMessageListView(viewRequest)

    // Extend the window: the base returns m1..m4. The store must be re-seeded
    // (the fix); without it the store still holds only m1, m2.
    await adapter.extendRuntimeSessionView({
      sessionId: 'sess',
      viewId: 'v1',
      count: 50,
    })

    // A message.updated for an original-row message arrives before the
    // broadcast viewReplace. The store re-projects; the emitted viewReplace
    // must contain the EXTENDED rows (m1..m4), not drop back to the first page
    // (the loadMore-vs-firehose race the override closes).
    built.harness.push(
      messageUpdated('m1', {
        id: 'm1',
        sourceId: 's',
        receivedAt: '2026-04-29T10:00:00Z',
        keywords: ['$flagged'],
        mailboxIds: ['inbox'],
        isRead: false,
        isFlagged: true,
        subject: 'm1',
      }),
    )
    await tick()

    const replace = [...frames].reverse().find((f) => f.type === 'viewReplace')
    expect(replace?.type).toBe('viewReplace')
    const rows =
      replace?.type === 'viewReplace' ? replace.snapshot.data.rows : []
    expect(rows.map((r) => (r.projection as { id: string }).id)).toEqual([
      'm1',
      'm2',
      'm3',
      'm4',
    ])
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
    await tick()

    const mailboxes = queryClient.getQueryData<Mailbox[]>(
      queryKeys.mailboxes('s'),
    )
    expect(mailboxes?.find((m) => m.id === 'inbox')?.unreadEmails).toBe(1)
  })

  it('exposes never-dispatched records to the engine reconciler + links receipts (D44a)', async () => {
    const { adapter, outbox, hooks, harness } = build()
    // A record optimistically accepted on a prior session but never dispatched
    // (runtimeMutationId === null), carrying its original send for replay —
    // plus a sent record and a request-less legacy record the replay hook
    // must NOT expose.
    await outbox.put({
      clientMutationId: 'c-orphan',
      messageId: 'm1',
      assertion: { kind: 'setKeywords', add: ['$flagged'], remove: [] },
      runtimeMutationId: null,
      acceptedAt: 1,
      request: setFlagged('m1', 'c-orphan'),
    })
    await outbox.put({
      clientMutationId: 'c-sent',
      messageId: 'm1',
      assertion: { kind: 'setKeywords', add: ['$flagged'], remove: [] },
      runtimeMutationId: 'r-prev',
      acceptedAt: 1,
      sessionId: 'sess-old',
      request: setFlagged('m1', 'c-sent'),
    })
    await outbox.put({
      clientMutationId: 'c-legacy',
      messageId: 'm2',
      assertion: { kind: 'setKeywords', add: ['$flagged'], remove: [] },
      runtimeMutationId: null,
      acceptedAt: 2,
    })
    await adapter.openRuntimeSessionMessageListView(viewRequest)

    // View open no longer resends anything — the trigger is DELETED (D44a);
    // the engine's connect-time reconciler drives replay through the hooks.
    expect(harness.mutations).toHaveLength(0)
    const replayable = await hooks().neverDispatched()
    expect(replayable.map((r) => r.clientMutationId)).toEqual(['c-orphan'])

    // A successful replay reports back: the receipt is linked (with the
    // session it was re-sent under), so the record leaves the replay set.
    await hooks().onReconciled(
      {
        runtimeMutationId: 'r-1',
        clientMutationId: 'c-orphan',
        name: 'message.setKeywords',
        state: 'accepted',
        error: null,
      },
      'sess-new',
    )
    const record = (await outbox.all()).find(
      (r) => r.clientMutationId === 'c-orphan',
    )
    expect(record?.runtimeMutationId).toBe('r-1')
    expect(record?.sessionId).toBe('sess-new')
    expect(await hooks().neverDispatched()).toHaveLength(0)
  })

  it('a terminal settlement from the reconciler settles optimism + clears the record (D44b)', async () => {
    const built = build()
    const { adapter, outbox, frames, hooks } = built
    await adapter.openRuntimeSessionMessageListView(viewRequest)

    // A live mutation: optimism folded, receipt linked under the live session.
    await adapter.runRuntimeMutation(setFlagged('m1', 'c1'))
    expect(keywordsOf(frames, 'm1')).toContain('$flagged')
    expect((await outbox.all())[0]?.sessionId).toBe('sess-live')

    // The engine's settlement query found a terminal FAILED verdict for it
    // (session-continuity loss): the optimism reverts and the record clears.
    await hooks().onSettlement({
      runtimeMutationId: 'r-1',
      clientMutationId: 'c1',
      name: 'message.setKeywords',
      state: 'failed',
      error: null,
    })
    await tick()

    expect(await outbox.all()).toHaveLength(0)
    expect(keywordsOf(frames, 'm1')).not.toContain('$flagged')
  })

  it('passes control operations (no local fold effect) straight through', async () => {
    // `revCursor` is the live control operation: a valid MailOperation variant
    // with no message target and no fold effect — the adapter forwards it
    // without touching the outbox or the optimistic store.
    const { adapter, outbox, harness } = build()
    await adapter.openRuntimeSessionMessageListView(viewRequest)
    await adapter.runRuntimeMutation({
      sessionId: 'sess',
      name: 'revCursor',
      args: { accountId: 's', cursorStepId: null, redoTail: [] },
      clientMutationId: 'c9',
    })
    expect(harness.mutations.map((m) => m.name)).toEqual(['revCursor'])
    expect(await outbox.all()).toHaveLength(0)
  })

  it('rejects operations outside the typed vocabulary at the wasm parse', async () => {
    // Post-M5 the operation vocabulary is closed: an unknown name is not a
    // pass-through — it fails the typed `MutationRequest` parse at the client
    // edge (the same rejection every wire crossing applies, D8/III).
    const { adapter, harness } = build()
    await adapter.openRuntimeSessionMessageListView(viewRequest)
    await expect(
      adapter.runRuntimeMutation({
        sessionId: 'sess',
        name: 'account.sync',
        args: { sourceId: 's' },
        clientMutationId: 'c9',
      }),
    ).rejects.toThrow()
    expect(harness.mutations).toHaveLength(0)
  })

  it('records an undo step only for user-initiated mutations (Phase 2 Slice 5d)', async () => {
    // Only mutations tagged `context.userInitiated` (explicit user gestures:
    // archive, flag, move) record an undo step. Internal/side-effect mutations
    // (e.g. auto-mark-read) omit the tag + don't pollute the undo history.
    const store = new MemoryUndoHistoryStore()
    setUndoHistoryStoreForTesting(store)
    try {
      const { adapter } = build()
      await adapter.openRuntimeSessionMessageListView(viewRequest) // seed replica

      // A user-initiated flag toggle records a step.
      await adapter.runRuntimeMutation({
        sessionId: 'sess',
        name: 'message.setKeywords',
        args: {
          sourceId: 's',
          messageId: 'm1',
          command: { add: ['$flagged'], remove: [] },
        },
        clientMutationId: 'c-u1',
        context: { userInitiated: true },
      })
      expect(store.canUndo()).toBe(true)
      expect(store.snapshot('s').steps).toHaveLength(1)

      // An internal setKeywords (no userInitiated — e.g. auto-mark-read) does NOT.
      await adapter.runRuntimeMutation({
        sessionId: 'sess',
        name: 'message.setKeywords',
        args: {
          sourceId: 's',
          messageId: 'm2',
          command: { add: ['$seen'], remove: [] },
        },
        clientMutationId: 'c-u2',
      })
      expect(store.snapshot('s').steps).toHaveLength(1) // still just the one
    } finally {
      resetUndoHistoryStoreForTesting()
    }
  })

  it('forwards unrelated frames unchanged (parity)', async () => {
    const built = build()
    const { adapter, frames } = built
    await adapter.openRuntimeSessionMessageListView(viewRequest)
    built.harness.push({ type: 'heartbeat', sessionSeq: 3 })
    expect(frames.at(-1)).toEqual({ type: 'heartbeat', sessionSeq: 3 })
  })
})
