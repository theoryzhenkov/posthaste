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
    makeHandle: () => makeRealHandle(),
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
    await Promise.resolve()
    await Promise.resolve()

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
    await Promise.resolve()
    await Promise.resolve()
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
    await Promise.resolve()
    await Promise.resolve()
    expect(keywordsOf(frames, 'm1')).toContain('$flagged')
    expect(await outbox.all()).toHaveLength(0)
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
