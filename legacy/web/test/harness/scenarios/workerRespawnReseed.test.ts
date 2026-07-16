/**
 * Scenario — CL-C1: a worker respawn must RE-SEED the store, not resurrect an
 * empty one.
 *
 * `WorkerStorePort.onTimeout` terminates a wedged worker and respawns a fresh
 * one — but a fresh worker starts with a BRAND-NEW empty `EntityStoreHandle`: no
 * registered views, no seeded bases, no folded optimism. Blind-replaying the
 * single timed-out call into that emptiness "succeeds" on nothing →
 * `drainAndEmit` emits a row-dropping `viewReplace` and unsettled optimistic
 * folds vanish from the read model. The comment framed replay as
 * "slow-but-eventually-answered" — wrong: the worker holds all the view/fold
 * state, so replay-into-empty is silent data loss reported as success.
 *
 * The fix drives the controller's re-seed hook on respawn: every open view is
 * re-registered + re-seeded from its held snapshot and the whole pending set is
 * re-folded BEFORE the timed-out call is replayed. This exercises the FULL stack
 * (real `WorkerStorePort` + real wasm store on both workers + the controller's
 * re-seed) and asserts the read model survives the respawn — the honest
 * end-to-end guarantee the old `workerWedgeRestart` scenario left as a
 * PORT-level replay proof.
 *
 * @spec docs/eph/RFC-L2-client-resilience.md (F3, M42)
 */
import { afterEach, describe, expect, it } from 'bun:test'

import { createClientHarness, messageUpdatedFrame } from '../index'
import type { ClientHarness } from '../index'
import type {
  RuntimeFrame,
  RuntimeMailListViewState,
  RuntimeRunMutationRequest,
} from '../../../src/runtime/types'

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

/** The rows (id + isFlagged) of the latest projected view frame. */
function latestRows(
  frames: RuntimeFrame<RuntimeMailListViewState>[],
): { id: string; isFlagged: boolean }[] | undefined {
  const last = [...frames]
    .reverse()
    .find((f) => f.type === 'viewReplace' || f.type === 'viewSnapshot')
  if (last?.type !== 'viewReplace' && last?.type !== 'viewSnapshot') {
    return undefined
  }
  return last.snapshot.data.rows.map((r) => {
    const p = r.projection as { id: string; isFlagged: boolean }
    return { id: p.id, isFlagged: p.isFlagged }
  })
}

const markRead = (id: string, receivedAt: string, isFlagged = false) =>
  messageUpdatedFrame(id, {
    id,
    sourceId: 's',
    receivedAt,
    keywords: isFlagged ? ['$seen', '$flagged'] : ['$seen'],
    mailboxIds: ['inbox'],
    isRead: true,
    isFlagged,
    subject: id,
  })

let harness: ClientHarness | undefined

afterEach(() => {
  harness?.dispose()
  harness = undefined
})

describe('scenario CL-C1: worker respawn re-seeds the store', () => {
  it('re-seeds open views + re-folds pending optimism on respawn (no silent emptiness)', async () => {
    const h = await createClientHarness({
      store: 'worker',
      callTimeoutMs: 20,
      maxRestarts: 1,
      coalescer: 'synchronous',
    })
    harness = h

    // Seed the view (rows m1, m2) on worker #1.
    await h.openView()
    await h.flush()

    // Fold an optimistic flag on m1 — this persists a durable pending-set record
    // AND applies the fold in the worker store.
    await h.adapter.runRuntimeMutation(setFlagged('m1', 'c1'))
    await h.flush()

    // Baseline: the store holds both rows and m1 is optimistically flagged.
    const before = latestRows(h.frames)
    expect(before?.map((r) => r.id).sort()).toEqual(['m1', 'm2'])
    expect(before?.find((r) => r.id === 'm1')?.isFlagged).toBe(true)
    expect(await h.pendingSet.all()).toHaveLength(1)

    // Wedge worker #1, then drive a store op: a `message.updated` (mark m2 read).
    // Its ingest call never answers → the watchdog terminates + respawns a fresh
    // EMPTY worker → the controller re-seeds it → the call is replayed.
    h.wedgeWorker()
    h.emitFrame(markRead('m2', '2026-04-28T10:00:00Z'))
    await h.advance(40)
    // A follow-up drain to settle any trailing store op the replay enqueued.
    await h.flush()

    // The worker was respawned exactly once.
    expect(h.worker?.spawnCount()).toBe(2)

    // The read model SURVIVED the respawn: both rows are still present (not
    // silently emptied), m1's re-folded optimism is intact, and m2's update
    // (applied on the re-seeded store) landed.
    const after = latestRows(h.frames)
    expect(after).toBeDefined()
    expect(after?.map((r) => r.id).sort()).toEqual(['m1', 'm2'])
    expect(after?.find((r) => r.id === 'm1')?.isFlagged).toBe(true)

    // The durable pending record is untouched — the fold was re-applied, not
    // dropped.
    expect(await h.pendingSet.all()).toHaveLength(1)
  })
})
