import { describe, expect, it } from 'bun:test'

import type { OkResponse } from '../src/api/types'
import type {
  ReplicaAssertion,
  ReplicaHandle,
} from '../src/runtime/replica/handle'
import { createReplicaAdapter } from '../src/runtime/replica/replicaAdapter'
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

// --- A faithful in-TS replica handle (the WASM handle is covered separately by
// the smoke test); this exercises the controller's orchestration. ---

interface BaseRow {
  messageId: string
  projection: { id: string; keywords: string[]; mailboxIds: string[] }
}

class FakeHandle implements ReplicaHandle {
  private base: BaseRow[] = []
  private readonly pending = new Map<
    string,
    { messageId: string; assertion: ReplicaAssertion }
  >()

  ingestJson(rowsJson: string): void {
    this.base = JSON.parse(rowsJson) as BaseRow[]
  }

  acceptJson(acceptJson: string): void {
    const { mutationId, messageId, assertion } = JSON.parse(acceptJson) as {
      mutationId: string
      messageId: string
      assertion: ReplicaAssertion
    }
    this.pending.set(mutationId, { messageId, assertion })
  }

  settle(mutationId: string): boolean {
    const existed = this.pending.delete(mutationId)
    return existed // treat any retired op as a potential revert for the test
  }

  hasPending(): boolean {
    return this.pending.size > 0
  }

  projectJson(mailboxId?: string | null): string {
    const rows = this.base.flatMap((row) => {
      let keywords = [...row.projection.keywords]
      let mailboxIds = [...row.projection.mailboxIds]
      let destroyed = false
      for (const op of this.pending.values()) {
        if (op.messageId !== row.messageId) continue
        if (op.assertion.kind === 'setKeywords') {
          keywords = keywords.filter((k) => !op.assertion.remove.includes(k))
          keywords.push(
            ...op.assertion.add.filter((k) => !keywords.includes(k)),
          )
        } else if (op.assertion.kind === 'replaceMailboxes') {
          mailboxIds = [...op.assertion.mailboxIds]
        } else if (op.assertion.kind === 'applyDiff') {
          const diff = op.assertion.diff
          keywords = keywords.filter((k) => !diff.keywords.removed.includes(k))
          keywords.push(
            ...diff.keywords.added.filter((k) => !keywords.includes(k)),
          )
          mailboxIds = mailboxIds.filter(
            (m) => !diff.mailboxes.removed.includes(m),
          )
          mailboxIds.push(
            ...diff.mailboxes.added.filter((m) => !mailboxIds.includes(m)),
          )
        } else {
          destroyed = true
        }
      }
      if (destroyed) return []
      if (mailboxId != null && !mailboxIds.includes(mailboxId)) return []
      return [{ id: row.projection.id, keywords, mailboxIds }]
    })
    return JSON.stringify(rows)
  }
}

// --- A fake base adapter exposing the four overridden surfaces + a push. ---

function snapshot(
  rows: BaseRow[],
): RuntimeViewSnapshot<RuntimeMailListViewState> {
  return {
    viewId: 'v1',
    descriptor: { family: 'mailList', payload: {} },
    revision: 1,
    lifecycle: 'ready',
    readWatermark: null,
    coverage: { kind: 'complete' },
    data: {
      scope: null,
      projectionKind: 'message',
      sort: null,
      windowRequest: null,
      rows: rows.map((row) => ({
        rowKey: `k-${row.messageId}`,
        resourceRef: `message:s:${row.messageId}`,
        projection: row.projection as never,
        orderKey: row.messageId,
      })),
      continuation: {
        beforeCursor: null,
        afterCursor: null,
        hasBefore: false,
        hasAfter: false,
      },
      readWatermark: null,
      coverage: { kind: 'complete' },
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
    operation: { name: 'test' } as never,
  },
}

function row(id: string, keywords: string[] = []): BaseRow {
  return { messageId: id, projection: { id, keywords, mailboxIds: ['inbox'] } }
}

function setSeen(
  id: string,
  clientMutationId: string,
): RuntimeRunMutationRequest {
  return {
    sessionId: 'sess',
    name: 'message.setKeywords',
    args: {
      sourceId: 's',
      messageId: id,
      command: { add: ['$seen'], remove: [] },
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

function build() {
  const harness = makeBase([row('m1'), row('m2')])
  const outbox = new MemoryOutboxStore()
  const adapter = createReplicaAdapter({
    base: harness.base,
    makeHandle: () => new FakeHandle(),
    outbox,
    now: () => 1,
  })
  const frames: RuntimeFrame<RuntimeMailListViewState>[] = []
  adapter.subscribeRuntimeFrames(
    { sessionId: 'sess' },
    { onFrame: (f) => frames.push(f) },
  )
  return { adapter, outbox, frames, harness }
}

describe('replicaAdapter', () => {
  it('returns the served base as the initial optimistic snapshot', async () => {
    const { adapter } = build()
    const opened = await adapter.openRuntimeSessionMessageListView(viewRequest)
    expect(
      opened.snapshot.data.rows.map((r) => (r.projection as { id: string }).id),
    ).toEqual(['m1', 'm2'])
  })

  it('folds a message mutation optimistically and forwards the POST + outbox', async () => {
    const { adapter, outbox, frames, harness } = build()
    await adapter.openRuntimeSessionMessageListView(viewRequest)

    const receipt = await adapter.runRuntimeMutation(setSeen('m1', 'c1'))

    // Optimistic frame emitted before settlement.
    expect(keywordsOf(frames, 'm1')).toContain('$seen')
    // The mutation was forwarded to the runtime.
    expect(harness.mutations.map((m) => m.clientMutationId)).toEqual(['c1'])
    // Outbox holds the unconfirmed intent, linked to the runtime id.
    const records = await outbox.all()
    expect(records).toHaveLength(1)
    expect(records[0]?.runtimeMutationId).toBe('r-1')
    expect(receipt.clientMutationId).toBe('c1')
  })

  it('retires the outbox on confirmation, keeping the now-authoritative state', async () => {
    const built = build()
    const { adapter, outbox, frames } = built
    await adapter.openRuntimeSessionMessageListView(viewRequest)
    await adapter.runRuntimeMutation(setSeen('m1', 'c1'))

    // The runtime confirms by re-serving a base that now carries $seen, then
    // settling. Retiring the pending op leaves the authoritative base in place.
    built.harness.push({
      type: 'viewReplace',
      sessionSeq: 4,
      viewId: 'v1',
      revision: 2,
      snapshot: snapshot([row('m1', ['$seen']), row('m2')]),
    })
    built.harness.push({
      type: 'mutationSettlement',
      sessionSeq: 5,
      mutationId: 'r-1',
      state: {
        clientMutationId: 'c1',
        name: 'message.setKeywords',
        status: 'confirmed',
        error: null,
      },
    })
    await Promise.resolve()
    await Promise.resolve()

    expect(await outbox.all()).toHaveLength(0)
    expect(keywordsOf(frames, 'm1')).toContain('$seen')
  })

  it('reverts optimism and clears the outbox on failure', async () => {
    const built = build()
    const { adapter, outbox, frames } = built
    await adapter.openRuntimeSessionMessageListView(viewRequest)
    await adapter.runRuntimeMutation(setSeen('m1', 'c1'))
    expect(keywordsOf(frames, 'm1')).toContain('$seen')

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
    expect(keywordsOf(frames, 'm1')).not.toContain('$seen')
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

  it('re-folds optimism over a runtime-served base correction', async () => {
    const built = build()
    const { adapter, frames } = built
    await adapter.openRuntimeSessionMessageListView(viewRequest)
    await adapter.runRuntimeMutation(setSeen('m1', 'c1'))

    // The runtime re-serves a base (e.g. m2 archived away); optimism persists.
    built.harness.push({
      type: 'viewReplace',
      sessionSeq: 9,
      viewId: 'v1',
      revision: 2,
      snapshot: snapshot([row('m1')]),
    })

    const last = frames.at(-1)
    expect(last?.type).toBe('viewReplace')
    if (last?.type === 'viewReplace') {
      expect(
        last.snapshot.data.rows.map((r) => (r.projection as { id: string }).id),
      ).toEqual(['m1'])
    }
    expect(keywordsOf(frames, 'm1')).toContain('$seen')
  })

  it('reverts a confirmed optimism when the provider later corrects the base', async () => {
    // The full lifecycle: client optimistic -> runtime confirms (store apply) ->
    // provider rejects later -> runtime re-serves the reverted authoritative
    // base (emit_failure_base_correction). With its op already retired the
    // replica simply adopts the correction.
    const built = build()
    const { adapter, frames, outbox } = built
    await adapter.openRuntimeSessionMessageListView(viewRequest)
    await adapter.runRuntimeMutation(setSeen('m1', 'c1'))
    expect(keywordsOf(frames, 'm1')).toContain('$seen')

    // Stage 2: confirm — re-serve the overlay-folded base + settle; op retires.
    built.harness.push({
      type: 'viewReplace',
      sessionSeq: 4,
      viewId: 'v1',
      revision: 2,
      snapshot: snapshot([row('m1', ['$seen']), row('m2')]),
    })
    built.harness.push({
      type: 'mutationSettlement',
      sessionSeq: 5,
      mutationId: 'r-1',
      state: {
        clientMutationId: 'c1',
        name: 'message.setKeywords',
        status: 'confirmed',
        error: null,
      },
    })
    await Promise.resolve()
    await Promise.resolve()
    expect(await outbox.all()).toHaveLength(0)
    expect(keywordsOf(frames, 'm1')).toContain('$seen')

    // Stage 3: the provider rejects; the runtime re-serves the reverted base.
    built.harness.push({
      type: 'viewReplace',
      sessionSeq: 9,
      viewId: 'v1',
      revision: 3,
      snapshot: snapshot([row('m1'), row('m2')]),
    })
    expect(keywordsOf(frames, 'm1')).not.toContain('$seen')
  })

  it('consumes a runtime viewDelta into the served base', async () => {
    const built = build()
    const { adapter, frames } = built
    await adapter.openRuntimeSessionMessageListView(viewRequest)

    // The runtime sends a row-local delta: m1 gains $flagged (upsert, no
    // reorder). The replica folds it into its served base and re-projects.
    built.harness.push({
      type: 'viewDelta',
      sessionSeq: 7,
      viewId: 'v1',
      revision: 2,
      delta: {
        order: null,
        upserts: [
          {
            rowKey: 'k-m1',
            resourceRef: 'message:s:m1',
            projection: {
              id: 'm1',
              keywords: ['$flagged'],
              mailboxIds: ['inbox'],
            } as never,
            orderKey: 'm1',
          },
        ],
      },
    })

    expect(keywordsOf(frames, 'm1')).toContain('$flagged')
  })

  it('folds a message.applyDiff assertion optimistically and forwards it through the outbox', async () => {
    const { adapter, outbox, frames, harness } = build()
    await adapter.openRuntimeSessionMessageListView(viewRequest)

    const diff = {
      keywords: { added: ['$flagged'], removed: [] },
      mailboxes: { added: [], removed: [] },
    }
    const receipt = await adapter.runRuntimeMutation({
      sessionId: 'sess',
      name: 'message.applyDiff',
      args: {
        sourceId: 's',
        messageId: 'm1',
        diff,
      },
      clientMutationId: 'c1',
    })

    expect(keywordsOf(frames, 'm1')).toContain('$flagged')
    expect(harness.mutations.map((m) => m.clientMutationId)).toEqual(['c1'])
    expect(receipt.clientMutationId).toBe('c1')
    const records = await outbox.all()
    expect(records).toHaveLength(1)
    expect(records[0]?.runtimeMutationId).toBe('r-1')
  })

  it('forwards unrelated frames unchanged (parity)', async () => {
    const built = build()
    const { adapter, frames } = built
    await adapter.openRuntimeSessionMessageListView(viewRequest)
    built.harness.push({ type: 'heartbeat', sessionSeq: 3 })
    expect(frames.at(-1)).toEqual({ type: 'heartbeat', sessionSeq: 3 })
  })
})
