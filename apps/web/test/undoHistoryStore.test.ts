// Client-owned undo/redo history — Phase 1. The store holds a `RevStep[]` +
// cursor, durable alongside the outbox, and navigation is LOCAL (no per-step
// round trip): chained undo pops the cursor in-memory and returns each step to
// invert. This is the round-trip-free win the design doc targets
// (@spec docs/eph/DESIGN-L2-undo-redo-synced-history#the-model).
import { describe, expect, it } from 'bun:test'

import {
  MemoryUndoHistoryStore,
  type RevLogSnapshotWire,
  type RevStep,
  type UndoHistorySnapshot,
  _optimismConfigForTesting,
} from '../src/runtime/replica/undoHistoryStore'

function step(id: string, messageId = 'm1'): RevStep {
  return {
    id,
    messageId,
    sourceId: 'acc1',
    diff: {
      keywords: { added: [], removed: [] },
      mailboxes: { added: ['x'], removed: ['y'] },
    },
    createdAt: Date.now(),
  }
}

describe('undo history store (client-owned cursor)', () => {
  it('pushForward records a step; undo returns it and clears canUndo', async () => {
    const store = new MemoryUndoHistoryStore()
    await store.pushForward(step('a'))
    const snap = store.snapshot()
    expect(snap.cursor).toBe(0)
    expect(snap.steps).toHaveLength(1)
    const undone = await store.navigateUndo()
    expect(undone?.id).toBe('a')
    expect(store.snapshot().cursor).toBe(-1)
  })

  it('chained undo navigates locally (the round-trip-free win)', async () => {
    const store = new MemoryUndoHistoryStore()
    await store.pushForward(step('a'))
    await store.pushForward(step('b'))
    await store.pushForward(step('c'))
    // Three undos in a row, each a local pop — no frame/round trip between them.
    expect((await store.navigateUndo())?.id).toBe('c')
    expect((await store.navigateUndo())?.id).toBe('b')
    expect((await store.navigateUndo())?.id).toBe('a')
    expect(await store.navigateUndo()).toBeNull()
  })

  it('redo replays the undone step', async () => {
    const store = new MemoryUndoHistoryStore()
    await store.pushForward(step('a'))
    await store.pushForward(step('b'))
    await store.navigateUndo() // undo b
    expect((await store.navigateRedo())?.id).toBe('b')
    expect(await store.navigateRedo()).toBeNull()
  })

  it('a new forward action truncates the redo tail (classic redo-clear)', async () => {
    const store = new MemoryUndoHistoryStore()
    await store.pushForward(step('a'))
    await store.pushForward(step('b'))
    await store.navigateUndo() // cursor at a; b redoable
    await store.pushForward(step('c')) // truncates b, appends c
    const snap = store.snapshot()
    expect(snap.steps.map((s) => s.id)).toEqual(['a', 'c'])
    expect(await store.navigateRedo()).toBeNull() // b gone
  })

  it('subscribe notifies on every history change', async () => {
    const store = new MemoryUndoHistoryStore()
    const snaps: UndoHistorySnapshot[] = []
    store.subscribe((s) => snaps.push(s))
    await store.pushForward(step('a'))
    await store.navigateUndo()
    expect(snaps.map((s) => s.cursor)).toEqual([0, -1])
  })

  it('clear empties the history', async () => {
    const store = new MemoryUndoHistoryStore()
    await store.pushForward(step('a'))
    await store.clear()
    expect(store.snapshot().steps).toEqual([])
    expect(await store.navigateUndo()).toBeNull()
  })

  it('persists across reload (shared backing) — cursor + steps survive', async () => {
    const backing = { snapshot: null as UndoHistorySnapshot | null }
    const store1 = new MemoryUndoHistoryStore(backing)
    await store1.pushForward(step('a'))
    await store1.pushForward(step('b'))
    await store1.navigateUndo() // cursor at a
    // A fresh store on the same backing reloads the persisted snapshot.
    const store2 = new MemoryUndoHistoryStore(backing)
    const loaded = await store2.load()
    expect(loaded.steps.map((s) => s.id)).toEqual(['a', 'b'])
    expect(loaded.cursor).toBe(0) // cursor preserved across reload
  })
})

describe('undo history store — Phase 2 mirror (reconcileWithServer)', () => {
  /** Build a server `RevLogSnapshotWire` from steps (by id, seq = index+1). */
  function serverSnapshot(
    stepIds: string[],
    cursorStepId: string | null,
    redoTail: string[] = [],
  ): RevLogSnapshotWire {
    return {
      steps: stepIds.map((id, i) => ({
        stepId: id,
        seq: i + 1,
        messageId: 'm1',
        sourceId: 'acc1',
        diff: {
          keywords: { added: [], removed: [] },
          mailboxes: { added: ['x'], removed: [] },
        },
        createdAt: `2026-01-0${i + 1}T00:00:00Z`,
      })),
      cursor: { cursorStepId, redoTail },
    }
  }

  it('adopts an empty server snapshot (no pending move → clears local state)', async () => {
    // A persisted snapshot (no in-flight local move) is reconciled away by an
    // empty server snapshot — e.g. the history was cleared on another device.
    const backing = {
      snapshot: { steps: [step('a')], cursor: 0 } as UndoHistorySnapshot,
    }
    const store = new MemoryUndoHistoryStore(backing)
    await store.load()
    await store.reconcileWithServer(serverSnapshot([], null))
    expect(store.snapshot().steps).toEqual([])
    expect(store.snapshot().cursor).toBe(-1)
  })

  it('adopts the server steps + cursor (cross-device convergence)', async () => {
    const store = new MemoryUndoHistoryStore()
    // Local state is empty; the server has two steps from another device,
    // cursor at the latest (b).
    await store.reconcileWithServer(serverSnapshot(['a', 'b'], 'b'))
    const snap = store.snapshot()
    expect(snap.steps.map((s) => s.id)).toEqual(['a', 'b'])
    expect(snap.cursor).toBe(1) // b
    expect(await store.navigateUndo()).not.toBeNull() // b is undoable
  })

  it('derives cursor = -1 when the server cursor is null (all undone)', async () => {
    const store = new MemoryUndoHistoryStore()
    await store.reconcileWithServer(
      serverSnapshot(['a', 'b'], null, ['a', 'b']),
    )
    expect(store.snapshot().cursor).toBe(-1)
    expect(store.snapshot().steps.map((s) => s.id)).toEqual(['a', 'b'])
  })

  it('skips adoption while a local move is in-flight (optimism guard)', async () => {
    const store = new MemoryUndoHistoryStore()
    await store.pushForward(step('a')) // cursor at a; pendingConfirm = {a}
    // A STALE server snapshot arrives (the server hasn't confirmed the append
    // yet — empty log, null cursor). The mirror must NOT revert the optimistic
    // forward action.
    await store.reconcileWithServer(serverSnapshot([], null))
    const snap = store.snapshot()
    expect(snap.steps.map((s) => s.id)).toEqual(['a']) // preserved
    expect(snap.cursor).toBe(0) // not reverted to -1
  })

  it('skips a stale cursor after undo (does not revert the optimistic undo)', async () => {
    const store = new MemoryUndoHistoryStore()
    await store.pushForward(step('a'))
    await store.navigateUndo() // cursor → -1; pendingConfirm = {null}
    // Stale server cursor still at 'a' (revCursor not processed yet).
    await store.reconcileWithServer(serverSnapshot(['a'], 'a'))
    expect(store.snapshot().cursor).toBe(-1) // optimistic undo preserved
  })

  it('confirms when the server echoes the optimistic cursor', async () => {
    const store = new MemoryUndoHistoryStore()
    await store.pushForward(step('a')) // pendingConfirm = {a}
    // Stale first (server hasn't caught up) — skipped.
    await store.reconcileWithServer(serverSnapshot([], null))
    expect(store.snapshot().cursor).toBe(0) // still optimistic
    // Now the server confirms: cursor at a, step a present.
    await store.reconcileWithServer(serverSnapshot(['a'], 'a'))
    expect(store.snapshot().cursor).toBe(0)
    // pendingConfirm cleared: a subsequent divergent server cursor is adopted
    // (cross-device), not skipped.
    await store.reconcileWithServer(serverSnapshot(['a', 'b'], 'b'))
    expect(store.snapshot().cursor).toBe(1) // adopted b
  })

  it('converges to the server after the optimism timeout (lost revCursor)', async () => {
    const store = new MemoryUndoHistoryStore()
    const restore = _optimismConfigForTesting.timeoutMs
    _optimismConfigForTesting.timeoutMs = 0 // expire immediately
    try {
      await store.pushForward(step('a')) // pendingConfirm = {a, now}
      // A stale server cursor (revCursor was lost — server still at null).
      // With a 0ms timeout, the mirror converges instead of skipping.
      await store.reconcileWithServer(serverSnapshot([], null))
      expect(store.snapshot().cursor).toBe(-1) // converged to server
    } finally {
      _optimismConfigForTesting.timeoutMs = restore
    }
  })
})
