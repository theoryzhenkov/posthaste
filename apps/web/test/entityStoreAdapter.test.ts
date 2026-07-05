import { beforeAll, beforeEach, describe, expect, it } from 'bun:test'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

import { QueryClient } from '@tanstack/react-query'

import type { OkResponse } from '../src/api/types'
import type { Mailbox } from '../src/api/types'
import {
  __resetLiveStoreForTesting,
  getMailboxCounts,
} from '../src/live-store/store'
import { queryKeys } from '../src/queryKeys'
import {
  createEntityStoreAdapter,
  flushActiveEntityStore,
  foldOptimisticMailMutation,
  revertOptimisticMailMutation,
} from '../src/runtime/replica/entityStoreAdapter'
import type { EntityStoreHandle } from '../src/runtime/replica/handle'
import { MemoryPendingSetStore } from '../src/runtime/replica/pendingSetStore'
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
  RuntimeLinkViewRequest,
  RuntimeViewSnapshot,
} from '../src/runtime/types'
import type { NearEndPendingSetHooks } from '../src/runtime/nearEnd'

// The adapter test drives the REAL wasm EntityStore handle (not a TS re-impl of
// the engine), so the controller's orchestration is verified against the engine
// that ships. The wasm bundle is a committed artifact; load + initialize it once.
const wasmDir = join(import.meta.dir, '..', 'src', 'runtime', 'wasm')
let makeRealHandle: () => EntityStoreHandle

beforeAll(async () => {
  const wasmModulePath = join(wasmDir, 'posthaste_client_node_wasm.js')
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const mod = (await import(wasmModulePath)) as any
  // Bun: initialize synchronously from the binary (avoids the file:// fetch).
  mod.initSync({
    module: readFileSync(join(wasmDir, 'posthaste_client_node_wasm_bg.wasm')),
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
    openRuntimeLinkMessageListView: async () => ({
      viewId: 'v1',
      snapshot: snapshot(rows),
    }),
    extendRuntimeLinkView: async () => ({
      viewId: 'v1',
      snapshot: snapshot([
        ...rows,
        row('m3', '2026-04-27T10:00:00Z'),
        row('m4', '2026-04-26T10:00:00Z'),
      ]),
    }),
    closeRuntimeLinkView: async (): Promise<OkResponse> => ({ ok: true }),
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

const viewRequest: RuntimeLinkViewRequest = {
  linkId: 'sess',
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
    linkId: 'sess',
    name: 'message.setKeywords',
    args: {
      sourceId: 's',
      messageId: id,
      command: { add: ['$flagged'], remove: [] },
    },
    clientMutationId,
  }
}

/** The ids present in the latest projected view snapshot (row order). */
function rowIds(
  frames: RuntimeFrame<RuntimeMailListViewState>[],
): string[] | undefined {
  const last = [...frames]
    .reverse()
    .find((f) => f.type === 'viewReplace' || f.type === 'viewSnapshot')
  if (last?.type !== 'viewReplace' && last?.type !== 'viewSnapshot') {
    return undefined
  }
  return last.snapshot.data.rows.map((r) => (r.projection as { id: string }).id)
}

/**
 * A `message.deleteDraft` runtime mutation (D130). The optimistic fold keys on
 * the row's `messageId` (the blink); the stable `draftId` rides along for the
 * far node's live-Email resolution.
 */
function deleteDraft(
  messageId: string,
  clientMutationId: string,
  draftId = `stable-${messageId}`,
): RuntimeRunMutationRequest {
  return {
    linkId: 'sess',
    name: 'message.deleteDraft',
    args: { sourceId: 's', messageId, draftId },
    clientMutationId,
  }
}

/**
 * A `message.saveDraft` runtime mutation (M65/D130). Not locally foldable
 * (`fold_effect` = None): no optimistic blink, no pending-set record — the value
 * is the typed, idempotent forwarded path replacing the fire-and-forget POST.
 */
function saveDraft(
  messageId: string,
  clientMutationId: string,
): RuntimeRunMutationRequest {
  return {
    linkId: 'sess',
    name: 'message.saveDraft',
    args: {
      sourceId: 's',
      messageId,
      request: { to: [], cc: [], bcc: [], subject: 'hi', body: 'draft body' },
    },
    clientMutationId,
  }
}

/**
 * A `message.send` runtime mutation (M66). Its `fold_effect` is a Destroy on the
 * originating draft's row (the blink): the stable draft key rides as `messageId`
 * (the fold target), the full send payload as `request`. A parked/rejected
 * settlement RETURNS the draft — a parked send is not a confirmed send.
 */
function send(
  messageId: string,
  clientMutationId: string,
): RuntimeRunMutationRequest {
  return {
    linkId: 'sess',
    name: 'message.send',
    args: {
      sourceId: 's',
      messageId,
      request: {
        to: [{ name: null, email: 'to@example.com' }],
        cc: [],
        bcc: [],
        subject: 'hi',
        body: 'send body',
        inReplyTo: null,
        references: null,
        attachments: [],
      },
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
    linkSeq: 100,
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
  const pendingSet = new MemoryPendingSetStore()
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
  // The adapter registers its durable-pending-set hooks with the near-end engine at
  // construction (D44); the stub captures them so tests can drive the
  // reconciler's calls directly (the engine's TIMING is pinned by the
  // `nearEndEngine` suite over fake IO).
  const captured: { hooks: NearEndPendingSetHooks | null } = { hooks: null }
  const adapter = createEntityStoreAdapter({
    base: harness.base,
    makeHandle: () => makeRealHandle(),
    pendingSet,
    queryClient,
    now: () => 1,
    nearEnd: {
      setPendingSetHooks: (hooks) => {
        captured.hooks = hooks
      },
      linkId: () => 'sess-live',
    },
  })
  const hooks = () => {
    if (!captured.hooks)
      throw new Error('pending-set hooks were not registered')
    return captured.hooks
  }
  const frames: RuntimeFrame<RuntimeMailListViewState>[] = []
  adapter.subscribeRuntimeFrames(
    { linkId: 'sess' },
    { onFrame: (f) => frames.push(f) },
  )
  return { adapter, pendingSet, frames, harness, queryClient, hooks }
}

/** Drain all pending microtasks (the serialized store queue) via a macrotask. */
const tick = () => new Promise<void>((resolve) => setTimeout(resolve, 0))

describe('entityStoreAdapter', () => {
  // The live store is a module singleton; reset its slices between cases so a
  // prior test's mirrored counts/projections can't bleed into the next.
  beforeEach(() => {
    __resetLiveStoreForTesting()
  })

  it('returns the served base as the initial projected snapshot', async () => {
    const { adapter } = build()
    const opened = await adapter.openRuntimeLinkMessageListView(viewRequest)
    expect(
      opened.snapshot.data.rows.map((r) => (r.projection as { id: string }).id),
    ).toEqual(['m1', 'm2'])
  })

  it('folds a message mutation optimistically + forwards the POST + pending set', async () => {
    const { adapter, pendingSet, frames, harness } = build()
    await adapter.openRuntimeLinkMessageListView(viewRequest)

    const receipt = await adapter.runRuntimeMutation(setFlagged('m1', 'c1'))

    expect(keywordsOf(frames, 'm1')).toContain('$flagged')
    expect(harness.mutations.map((m) => m.clientMutationId)).toEqual(['c1'])
    const records = await pendingSet.all()
    expect(records).toHaveLength(1)
    expect(records[0]?.runtimeMutationId).toBe('r-1')
    expect(receipt.clientMutationId).toBe('c1')
  })

  it('reverts optimism + clears the pending set on failure', async () => {
    const built = build()
    const { adapter, pendingSet, frames } = built
    await adapter.openRuntimeLinkMessageListView(viewRequest)
    await adapter.runRuntimeMutation(setFlagged('m1', 'c1'))
    expect(keywordsOf(frames, 'm1')).toContain('$flagged')

    built.harness.push({
      type: 'mutationNotification',
      linkSeq: 5,
      clientMutationId: 'c1',
      notification: {
        type: 'rejected',
        error: {
          code: 'conflict',
          message: 'rejected by the authority',
          terminality: 'permanent',
        },
      },
    })
    await tick()

    expect(await pendingSet.all()).toHaveLength(0)
    expect(keywordsOf(frames, 'm1')).not.toContain('$flagged')
  })

  it('discard (D130): optimistically removes the draft row (the blink) + forwards the mutation + pending set', async () => {
    const { adapter, pendingSet, frames, harness } = build()
    const opened = await adapter.openRuntimeLinkMessageListView(viewRequest)
    expect(
      opened.snapshot.data.rows.map((r) => (r.projection as { id: string }).id),
    ).toEqual(['m1', 'm2'])

    const receipt = await adapter.runRuntimeMutation(deleteDraft('m1', 'd1'))

    // The blink: the draft row is gone from the projected view immediately.
    expect(rowIds(frames)).toEqual(['m2'])
    // It forwarded as a real runtime mutation (not a fire-and-forget POST),
    // carrying the stable draftId for far-node resolution.
    expect(harness.mutations.map((m) => m.name)).toEqual([
      'message.deleteDraft',
    ])
    expect(harness.mutations[0]?.args).toMatchObject({
      messageId: 'm1',
      draftId: 'stable-m1',
    })
    expect((await pendingSet.all()).map((r) => r.clientMutationId)).toEqual([
      'd1',
    ])
    expect(receipt.clientMutationId).toBe('d1')
  })

  it('discard (D130): a confirmed settlement KEEPS the row removed', async () => {
    const built = build()
    const { adapter, frames } = built
    await adapter.openRuntimeLinkMessageListView(viewRequest)
    await adapter.runRuntimeMutation(deleteDraft('m1', 'd1'))
    expect(rowIds(frames)).toEqual(['m2'])

    built.harness.push({
      type: 'mutationNotification',
      linkSeq: 5,
      clientMutationId: 'd1',
      notification: { type: 'confirmed' },
    })
    await tick()

    // Still gone; the reconciling message.updated{deleted:true} then prunes the
    // base authoritatively (the row never comes back).
    expect(rowIds(frames)).toEqual(['m2'])
    built.harness.push({
      type: 'notification',
      linkSeq: 101,
      kind: 'message.updated',
      payload: {
        seq: 2,
        accountId: 's',
        topic: 'message.updated',
        occurredAt: 'now',
        payload: { messageId: 'm1', deleted: true },
      },
    } as RuntimeFrame<RuntimeMailListViewState>)
    await tick()
    expect(rowIds(frames)).toEqual(['m2'])
  })

  it('discard (D130/D134): a rejected settlement REVERTS the blink + surfaces (not silent)', async () => {
    const built = build()
    const { adapter, pendingSet, frames } = built
    await adapter.openRuntimeLinkMessageListView(viewRequest)
    await adapter.runRuntimeMutation(deleteDraft('m1', 'd1'))
    // Optimistically removed.
    expect(rowIds(frames)).toEqual(['m2'])

    built.harness.push({
      type: 'mutationNotification',
      linkSeq: 5,
      clientMutationId: 'd1',
      notification: {
        type: 'rejected',
        error: {
          code: 'notFound',
          message: 'draft not found',
          terminality: 'permanent',
        },
      },
    })
    await tick()

    // The row comes back (the reverting settlement) and the pending op clears —
    // a user discard's failure is surfaced, never a silent success (the M60
    // regression this fixes).
    expect(rowIds(frames)).toEqual(['m1', 'm2'])
    expect(await pendingSet.all()).toHaveLength(0)
  })

  it('deferred discard (FIX1/D134): fold removes the row immediately, WITHOUT dispatching or a durable record', async () => {
    const { adapter, pendingSet, frames, harness } = build()
    const opened = await adapter.openRuntimeLinkMessageListView(viewRequest)
    expect(
      opened.snapshot.data.rows.map((r) => (r.projection as { id: string }).id),
    ).toEqual(['m1', 'm2'])

    const foldId = await foldOptimisticMailMutation(deleteDraft('m1', 'fold-1'))

    // The instant blink: the draft row is gone from the projected view.
    expect(foldId).toBe('fold-1')
    expect(rowIds(frames)).toEqual(['m2'])
    // Nothing hit the server, and no durable pending-set record was written —
    // a tab close during the grace drops the fold with the page (draft kept).
    expect(harness.mutations).toHaveLength(0)
    expect(await pendingSet.all()).toHaveLength(0)
  })

  it('deferred discard (FIX1/D134): revert restores the folded row with no server round-trip', async () => {
    const { adapter, pendingSet, frames, harness } = build()
    await adapter.openRuntimeLinkMessageListView(viewRequest)

    await foldOptimisticMailMutation(deleteDraft('m1', 'fold-1'))
    expect(rowIds(frames)).toEqual(['m2'])

    await revertOptimisticMailMutation('fold-1')

    // The row comes back purely client-side; nothing was dispatched.
    expect(rowIds(frames)).toEqual(['m1', 'm2'])
    expect(harness.mutations).toHaveLength(0)
    expect(await pendingSet.all()).toHaveLength(0)
  })

  it('deferred discard (FIX1/D134): committing under the SAME id dispatches once with no second blink + one durable record', async () => {
    const { adapter, pendingSet, frames, harness } = build()
    await adapter.openRuntimeLinkMessageListView(viewRequest)

    // Phase 1: fold NOW (the blink), client-only.
    await foldOptimisticMailMutation(deleteDraft('m1', 'fold-1'))
    expect(rowIds(frames)).toEqual(['m2'])

    // Phase 2 (grace elapsed): commit by re-running the SAME mutation under the
    // fold's id — idempotent re-fold (no second blink / no flip), plus dispatch
    // + exactly one durable record.
    const receipt = await adapter.runRuntimeMutation(
      deleteDraft('m1', 'fold-1'),
    )
    expect(rowIds(frames)).toEqual(['m2'])
    expect(harness.mutations.map((m) => m.name)).toEqual([
      'message.deleteDraft',
    ])
    expect((await pendingSet.all()).map((r) => r.clientMutationId)).toEqual([
      'fold-1',
    ])
    expect(receipt.clientMutationId).toBe('fold-1')

    // And a rejected settlement still reverts the fold + surfaces (M64): the row
    // returns and the record clears.
    harness.push({
      type: 'mutationNotification',
      linkSeq: 5,
      clientMutationId: 'fold-1',
      notification: {
        type: 'rejected',
        error: {
          code: 'notFound',
          message: 'draft not found',
          terminality: 'permanent',
        },
      },
    })
    await tick()
    expect(rowIds(frames)).toEqual(['m1', 'm2'])
    expect(await pendingSet.all()).toHaveLength(0)
  })

  it('save (M65/D130): forwards through the typed runMutation path with NO optimistic fold + NO pending-set record', async () => {
    const { adapter, pendingSet, frames, harness } = build()
    const opened = await adapter.openRuntimeLinkMessageListView(viewRequest)
    expect(
      opened.snapshot.data.rows.map((r) => (r.projection as { id: string }).id),
    ).toEqual(['m1', 'm2'])
    const framesBefore = frames.length

    const receipt = await adapter.runRuntimeMutation(
      saveDraft('draft-local-1', 's1'),
    )

    // A save has no expressible optimistic fold (the vocabulary has no upsert):
    // nothing is folded, so the store never re-projects — no view frame is
    // emitted (no blink).
    expect(frames.length).toBe(framesBefore)
    // It forwarded as a real, typed runtime mutation (not a fire-and-forget
    // POST), so redelivery dedups at the seam and errors surface.
    expect(harness.mutations.map((m) => m.name)).toEqual(['message.saveDraft'])
    expect(harness.mutations[0]?.args).toMatchObject({
      messageId: 'draft-local-1',
      request: { subject: 'hi' },
    })
    // Foldless ops carry no optimism to settle/revert → no pending-set record.
    expect(await pendingSet.all()).toHaveLength(0)
    expect(receipt.clientMutationId).toBe('s1')
  })

  it('send (M66): optimistically Destroys the originating draft row (the blink) + forwards + pending set', async () => {
    const { adapter, pendingSet, frames, harness } = build()
    const opened = await adapter.openRuntimeLinkMessageListView(viewRequest)
    expect(
      opened.snapshot.data.rows.map((r) => (r.projection as { id: string }).id),
    ).toEqual(['m1', 'm2'])

    const receipt = await adapter.runRuntimeMutation(send('m1', 'snd1'))

    // The blink: the originating draft row is gone from the projected view
    // immediately (the Destroy fold applied).
    expect(rowIds(frames)).toEqual(['m2'])
    // It forwarded as a real, typed runtime mutation (not a fire-and-forget
    // POST), carrying the stable draft key + the send payload.
    expect(harness.mutations.map((m) => m.name)).toEqual(['message.send'])
    expect(harness.mutations[0]?.args).toMatchObject({
      messageId: 'm1',
      request: { subject: 'hi' },
    })
    // A folded op carries optimism to settle/revert → exactly one pending-set
    // record survives (the durable outbox intent).
    expect((await pendingSet.all()).map((r) => r.clientMutationId)).toEqual([
      'snd1',
    ])
    expect(receipt.clientMutationId).toBe('snd1')
  })

  it('send (M66): an Applied CONFIRMS — the draft stays gone, no false Sent flicker', async () => {
    const built = build()
    const { adapter, frames } = built
    await adapter.openRuntimeLinkMessageListView(viewRequest)
    await adapter.runRuntimeMutation(send('m1', 'snd1'))
    expect(rowIds(frames)).toEqual(['m2'])

    built.harness.push({
      type: 'mutationNotification',
      linkSeq: 5,
      clientMutationId: 'snd1',
      notification: { type: 'confirmed' },
    })
    await tick()

    // Still gone; the send Applied, so the reconciling message.updated then
    // prunes the base authoritatively. No false "Sent then flicker back".
    expect(rowIds(frames)).toEqual(['m2'])
    built.harness.push({
      type: 'notification',
      linkSeq: 101,
      kind: 'message.updated',
      payload: {
        seq: 2,
        accountId: 's',
        topic: 'message.updated',
        occurredAt: 'now',
        payload: { messageId: 'm1', deleted: true },
      },
    } as RuntimeFrame<RuntimeMailListViewState>)
    await tick()
    expect(rowIds(frames)).toEqual(['m2'])
  })

  it('send (DS8): a DispatchUncertain/park => Rejected settlement RETURNS the draft (a parked send is not a confirmed send)', async () => {
    const built = build()
    const { adapter, pendingSet, frames } = built
    await adapter.openRuntimeLinkMessageListView(viewRequest)
    await adapter.runRuntimeMutation(send('m1', 'snd1'))
    // The blink: optimistically Destroyed.
    expect(rowIds(frames)).toEqual(['m2'])

    built.harness.push({
      type: 'mutationNotification',
      linkSeq: 5,
      clientMutationId: 'snd1',
      notification: {
        type: 'rejected',
        error: {
          code: 'dispatchUncertain',
          message: 'send parked; delivery outcome unknown',
          terminality: 'permanent',
        },
      },
    })
    await tick()

    // The draft comes back (the reverting settlement) and the pending op clears.
    // A parked send is surfaced as needs-attention, never a silent "Sent".
    expect(rowIds(frames)).toEqual(['m1', 'm2'])
    expect(await pendingSet.all()).toHaveLength(0)
  })

  it('send (DS8): a resolved failed receipt reverts the blink immediately (state, not id-presence)', async () => {
    const built = build()
    const { adapter, pendingSet, frames, harness } = built
    await adapter.openRuntimeLinkMessageListView(viewRequest)

    // The runtime resolves the dispatch ALREADY terminally failed (state, not a
    // throw) — and hands back a runtimeMutationId too, to prove the adapter
    // checks `state` rather than just id-presence (which would mis-link + park).
    harness.base.runRuntimeMutation = async (
      request: RuntimeRunMutationRequest,
    ) => {
      harness.mutations.push(request)
      return {
        runtimeMutationId: 'r-fail',
        clientMutationId: request.clientMutationId,
        name: request.name,
        state: 'failed',
        error: {
          code: 'refused',
          message: 'send refused',
          terminality: 'permanent',
        },
      } satisfies RuntimeMutationReceipt
    }

    const receipt = await adapter.runRuntimeMutation(send('m1', 'snd1'))

    // WITHOUT pushing any settlement frame: the blink is already reverted (the
    // draft is back) and the pending op cleared — the terminal receipt settled it.
    expect(receipt.state).toBe('failed')
    expect(rowIds(frames)).toEqual(['m1', 'm2'])
    expect(await pendingSet.all()).toHaveLength(0)
  })

  it('send (M66): idempotent redelivery = one durable send (same clientMutationId dedups)', async () => {
    const { adapter, pendingSet, frames, harness } = build()
    await adapter.openRuntimeLinkMessageListView(viewRequest)

    // Redelivery: the SAME send (same clientMutationId) runs twice — a retried
    // forward, not a second message.
    const r1 = await adapter.runRuntimeMutation(send('m1', 'snd1'))
    const r2 = await adapter.runRuntimeMutation(send('m1', 'snd1'))

    // One blink, not a double-fold artifact: the draft is Destroyed once.
    expect(rowIds(frames)).toEqual(['m2'])
    // Both receipts key to the same client mutation id: redelivery is the same
    // send. The durable outbox dedups to exactly ONE record (the seam the far
    // node's exactly-once dedup keys on), never two competing sends.
    expect(r1.clientMutationId).toBe('snd1')
    expect(r2.clientMutationId).toBe('snd1')
    expect((await pendingSet.all()).map((r) => r.clientMutationId)).toEqual([
      'snd1',
    ])
    // Every forwarded op carries that same stable id (so a redelivery is
    // dedup-able downstream), never a fresh per-attempt id.
    expect(harness.mutations.map((m) => m.name)).toEqual([
      'message.send',
      'message.send',
    ])
    expect(harness.mutations.map((m) => m.clientMutationId)).toEqual([
      'snd1',
      'snd1',
    ])
  })

  it('surfaces a clear error + reverts the optimistic fold when the durable write fails (W4 quota)', async () => {
    const built = build()
    const { adapter, pendingSet, frames } = built
    await adapter.openRuntimeLinkMessageListView(viewRequest)

    const originalPut = pendingSet.put.bind(pendingSet)
    // Simulate IndexedDB's QuotaExceededError: the browser's storage quota is
    // exhausted, so the durable pending-set write rejects.
    pendingSet.put = () => {
      throw new DOMException(
        'The quota has been exceeded.',
        'QuotaExceededError',
      )
    }

    await expect(
      adapter.runRuntimeMutation(setFlagged('m1', 'c1')),
    ).rejects.toThrow(/storage is full/i)

    // Nothing durable was left stranded: the fold was reverted before it was
    // ever persisted, so there's no orphaned pending-set record.
    expect(await pendingSet.all()).toHaveLength(0)

    // The store must not be left corrupted with an un-settleable optimistic
    // fold, either: it accepts + folds + persists a normal follow-up mutation
    // on the SAME message cleanly.
    pendingSet.put = originalPut
    const receipt = await adapter.runRuntimeMutation(setFlagged('m1', 'c2'))
    expect(receipt.clientMutationId).toBe('c2')
    expect(keywordsOf(frames, 'm1')).toContain('$flagged')
    expect((await pendingSet.all()).map((r) => r.clientMutationId)).toEqual([
      'c2',
    ])
  })

  it('confirmed before the base update does not revert (flicker fix)', async () => {
    const built = build()
    const { adapter, pendingSet, frames } = built
    await adapter.openRuntimeLinkMessageListView(viewRequest)
    await adapter.runRuntimeMutation(setFlagged('m1', 'c1'))
    expect(keywordsOf(frames, 'm1')).toContain('$flagged')

    // The confirmation outruns the authoritative message.updated. It must NOT
    // flip the row back to the un-flagged base; the durable pending set RETAINS the
    // op (outbox D: not retired yet — the base hasn't caught up to absorb it).
    built.harness.push({
      type: 'mutationNotification',
      linkSeq: 5,
      clientMutationId: 'c1',
      notification: { type: 'confirmed' },
    })
    await tick()
    expect(keywordsOf(frames, 'm1')).toContain('$flagged')
    expect(await pendingSet.all()).toHaveLength(1)

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
    expect(await pendingSet.all()).toHaveLength(0)
  })

  it('rehydration keeps reconcilable sent records and drops legacy ones', async () => {
    const { adapter, pendingSet, hooks } = build()
    // A prior link left durable records: a sent one WITH its dispatch
    // link (the engine's reconciler can query its settlement — D44b, keep),
    // a legacy sent one WITHOUT (unqueryable → dropped, the old leak guard),
    // and a never-sent one (still-unconfirmed intent that must survive).
    await pendingSet.put({
      clientMutationId: 'sent-reconcilable',
      messageId: 'm1',
      assertion: { kind: 'setKeywords', add: ['$flagged'], remove: [] },
      runtimeMutationId: 'r-prev',
      acceptedAt: 1,
      linkId: 'sess-old',
      request: setFlagged('m1', 'sent-reconcilable'),
    })
    await pendingSet.put({
      clientMutationId: 'sent-legacy',
      messageId: 'm1',
      assertion: { kind: 'setKeywords', add: ['$flagged'], remove: [] },
      runtimeMutationId: 'r-prev-2',
      acceptedAt: 1,
    })
    await pendingSet.put({
      clientMutationId: 'never-sent-1',
      messageId: 'm2',
      assertion: { kind: 'setKeywords', add: ['$flagged'], remove: [] },
      runtimeMutationId: null,
      acceptedAt: 1,
    })

    await adapter.openRuntimeLinkMessageListView(viewRequest)

    const remaining = await pendingSet.all()
    expect(remaining.map((r) => r.clientMutationId).sort()).toEqual([
      'never-sent-1',
      'sent-reconcilable',
    ])
    // The reconcilable record is exactly what the engine's sent-but-unsettled
    // query sees, keyed to its ORIGINAL link.
    const unsettled = await hooks().sentUnsettled()
    expect(unsettled).toEqual([
      {
        linkId: 'sess-old',
        clientMutationId: 'sent-reconcilable',
        request: setFlagged('m1', 'sent-reconcilable'),
      },
    ])
  })

  it('ingests a message.updated notification and re-projects the row', async () => {
    const built = build()
    const { adapter, frames } = built
    await adapter.openRuntimeLinkMessageListView(viewRequest)

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

  it('flush() awaits a fire-and-forget queued store op (W3 unload durability)', async () => {
    const built = build()
    const { adapter, frames } = built
    await adapter.openRuntimeLinkMessageListView(viewRequest)

    // A notification frame enqueues its store op fire-and-forget (`void
    // this.enqueue(...)`) — nothing in the caller awaits it.
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
    // Immediately after the push, the queued re-projection hasn't run yet
    // (nothing emitted to the frame sink) — this is the race a bare
    // page-close could land in without a flush.
    expect(frames).toHaveLength(0)

    await flushActiveEntityStore()

    // flush() guarantees the queued op (and any durable write inside it) has
    // completed before it returns.
    expect(frames.length).toBeGreaterThan(0)
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
      pendingSet: new MemoryPendingSetStore(),
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
      { linkId: 'sess' },
      { onFrame: (f) => frames.push(f) },
    )
    await adapter.openRuntimeLinkMessageListView(viewRequest)
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
      pendingSet: new MemoryPendingSetStore(),
      now: () => 1,
      scheduleFlush: (cb) => {
        scheduledFlush = cb
        return () => {
          scheduledFlush = null
        }
      },
    })
    adapter.subscribeRuntimeFrames({ linkId: 'sess' }, { onFrame: () => {} })
    await adapter.openRuntimeLinkMessageListView(viewRequest)

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
    await adapter.openRuntimeLinkMessageListView(viewRequest)

    // Extend the window: the base returns m1..m4. The store must be re-seeded
    // (the fix); without it the store still holds only m1, m2.
    await adapter.extendRuntimeLinkView({
      linkId: 'sess',
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

  it('mirrors the count delta into the live store slice, NOT react-query (D116)', async () => {
    const built = build()
    const { adapter, queryClient } = built
    await adapter.openRuntimeLinkMessageListView(viewRequest)

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

    // The count lands in the store's counts slice (the sidebar's read model).
    expect(getMailboxCounts('s').inbox).toEqual({ unread: 1, total: 2 })
    // react-query's mailbox row is NOT touched — live counts are no longer
    // request/response cache state (the setQueryData-for-counts path is gone).
    const mailboxes = queryClient.getQueryData<Mailbox[]>(
      queryKeys.mailboxes('s'),
    )
    expect(mailboxes?.find((m) => m.id === 'inbox')?.unreadEmails).toBe(2)
  })

  it('exposes never-dispatched records to the engine reconciler + links receipts (D44a)', async () => {
    const { adapter, pendingSet, hooks, harness } = build()
    // A record optimistically accepted on a prior link but never dispatched
    // (runtimeMutationId === null), carrying its original send for replay —
    // plus a sent record and a request-less legacy record the replay hook
    // must NOT expose.
    await pendingSet.put({
      clientMutationId: 'c-orphan',
      messageId: 'm1',
      assertion: { kind: 'setKeywords', add: ['$flagged'], remove: [] },
      runtimeMutationId: null,
      acceptedAt: 1,
      request: setFlagged('m1', 'c-orphan'),
    })
    await pendingSet.put({
      clientMutationId: 'c-sent',
      messageId: 'm1',
      assertion: { kind: 'setKeywords', add: ['$flagged'], remove: [] },
      runtimeMutationId: 'r-prev',
      acceptedAt: 1,
      linkId: 'sess-old',
      request: setFlagged('m1', 'c-sent'),
    })
    await pendingSet.put({
      clientMutationId: 'c-legacy',
      messageId: 'm2',
      assertion: { kind: 'setKeywords', add: ['$flagged'], remove: [] },
      runtimeMutationId: null,
      acceptedAt: 2,
    })
    await adapter.openRuntimeLinkMessageListView(viewRequest)

    // View open no longer resends anything — the trigger is DELETED (D44a);
    // the engine's connect-time reconciler drives replay through the hooks.
    expect(harness.mutations).toHaveLength(0)
    const replayable = await hooks().neverDispatched()
    expect(replayable.map((r) => r.clientMutationId)).toEqual(['c-orphan'])

    // A successful replay reports back: the receipt is linked (with the
    // link it was re-sent under), so the record leaves the replay set.
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
    const record = (await pendingSet.all()).find(
      (r) => r.clientMutationId === 'c-orphan',
    )
    expect(record?.runtimeMutationId).toBe('r-1')
    expect(record?.linkId).toBe('sess-new')
    expect(await hooks().neverDispatched()).toHaveLength(0)
  })

  it('a terminal settlement from the reconciler settles optimism + clears the record (D44b)', async () => {
    const built = build()
    const { adapter, pendingSet, frames, hooks } = built
    await adapter.openRuntimeLinkMessageListView(viewRequest)

    // A live mutation: optimism folded, receipt linked under the live link.
    await adapter.runRuntimeMutation(setFlagged('m1', 'c1'))
    expect(keywordsOf(frames, 'm1')).toContain('$flagged')
    expect((await pendingSet.all())[0]?.linkId).toBe('sess-live')

    // The engine's settlement query found a terminal FAILED verdict for it
    // (link-continuity loss): the optimism reverts and the record clears.
    await hooks().onSettlement({
      runtimeMutationId: 'r-1',
      clientMutationId: 'c1',
      name: 'message.setKeywords',
      state: 'failed',
      error: null,
    })
    await tick()

    expect(await pendingSet.all()).toHaveLength(0)
    expect(keywordsOf(frames, 'm1')).not.toContain('$flagged')
  })

  it('passes control operations (no local fold effect) straight through', async () => {
    // `revCursor` is the live control operation: a valid MailOperation variant
    // with no message target and no fold effect — the adapter forwards it
    // without touching the pending set or the optimistic store.
    const { adapter, pendingSet, harness } = build()
    await adapter.openRuntimeLinkMessageListView(viewRequest)
    await adapter.runRuntimeMutation({
      linkId: 'sess',
      name: 'revCursor',
      args: { accountId: 's', cursorStepId: null, redoTail: [] },
      clientMutationId: 'c9',
    })
    expect(harness.mutations.map((m) => m.name)).toEqual(['revCursor'])
    expect(await pendingSet.all()).toHaveLength(0)
  })

  it('rejects operations outside the typed vocabulary at the wasm parse', async () => {
    // Post-M5 the operation vocabulary is closed: an unknown name is not a
    // pass-through — it fails the typed `MutationRequest` parse at the client
    // edge (the same rejection every wire crossing applies, D8/III).
    const { adapter, harness } = build()
    await adapter.openRuntimeLinkMessageListView(viewRequest)
    await expect(
      adapter.runRuntimeMutation({
        linkId: 'sess',
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
      await adapter.openRuntimeLinkMessageListView(viewRequest) // seed replica

      // A user-initiated flag toggle records a step.
      await adapter.runRuntimeMutation({
        linkId: 'sess',
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
        linkId: 'sess',
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
    await adapter.openRuntimeLinkMessageListView(viewRequest)
    built.harness.push({ type: 'heartbeat', linkSeq: 3 })
    expect(frames.at(-1)).toEqual({ type: 'heartbeat', linkSeq: 3 })
  })
})
