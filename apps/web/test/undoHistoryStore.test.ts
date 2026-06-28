// Client-owned undo/redo history — Phase 2 (multi-account). The store holds a
// per-account `RevStep[]` + cursor partition (the singleton mixing of Phase 1 is
// gone). Navigation is LOCAL (no per-step round trip): chained undo pops the
// cursor in-memory + returns each step to invert. The global Ctrl+Z merges
// per-account histories by `createdAt` (the latest undoable step across all
// accounts) — no globally-ordered log needed. The store is also the mirror of
// the server-authoritative `RevLog` view (`reconcileWithServer`).
//
// @spec docs/eph/DESIGN-L2-undo-redo-revlog-contract
import { describe, expect, it } from 'bun:test'

import {
  MemoryUndoHistoryStore,
  type RevLogSnapshotWire,
  type RevStep,
  type UndoHistorySnapshot,
  _optimismConfigForTesting,
} from '../src/runtime/replica/undoHistoryStore'

const DIFF = {
  keywords: { added: [], removed: [] },
  mailboxes: { added: ['x'], removed: ['y'] },
}

function step(id: string, accountId = 'acc1', createdAt = 1): RevStep {
  return {
    id,
    messageId: 'm1',
    sourceId: accountId,
    diff: DIFF,
    createdAt,
  }
}

describe('undo history store (per-account + global merge)', () => {
  it('pushForward records a step on an account; undo returns it', async () => {
    const store = new MemoryUndoHistoryStore()
    await store.pushForward('acc1', step('a'))
    const snap = store.snapshot('acc1')
    expect(snap.cursor).toBe(0)
    expect(snap.steps).toHaveLength(1)
    const undone = await store.undo()
    expect(undone?.step.id).toBe('a')
    expect(undone?.accountId).toBe('acc1')
    expect(store.snapshot('acc1').cursor).toBe(-1)
  })

  it('chained undo navigates locally (the round-trip-free win)', async () => {
    const store = new MemoryUndoHistoryStore()
    await store.pushForward('acc1', step('a', 'acc1', 1))
    await store.pushForward('acc1', step('b', 'acc1', 2))
    await store.pushForward('acc1', step('c', 'acc1', 3))
    // Three undos in a row, each a local pop — no frame/round trip between them.
    expect((await store.undo())?.step.id).toBe('c')
    expect((await store.undo())?.step.id).toBe('b')
    expect((await store.undo())?.step.id).toBe('a')
    expect(await store.undo()).toBeNull()
  })

  it('redo replays the undone step', async () => {
    const store = new MemoryUndoHistoryStore()
    await store.pushForward('acc1', step('a', 'acc1', 1))
    await store.pushForward('acc1', step('b', 'acc1', 2))
    await store.undo() // undo b
    expect((await store.redo())?.step.id).toBe('b')
    expect(await store.redo()).toBeNull()
  })

  it('a new forward action truncates the redo tail (classic redo-clear)', async () => {
    const store = new MemoryUndoHistoryStore()
    await store.pushForward('acc1', step('a', 'acc1', 1))
    await store.pushForward('acc1', step('b', 'acc1', 2))
    await store.undo() // cursor at a; b redoable
    await store.pushForward('acc1', step('c', 'acc1', 3)) // truncates b, appends c
    const snap = store.snapshot('acc1')
    expect(snap.steps.map((s) => s.id)).toEqual(['a', 'c'])
    expect(await store.redo()).toBeNull() // b gone
  })

  it('subscribe notifies on every history change', async () => {
    const store = new MemoryUndoHistoryStore()
    const events: number[] = []
    store.subscribe(() => events.push(events.length))
    await store.pushForward('acc1', step('a'))
    await store.undo()
    expect(events).toHaveLength(2)
  })

  it('clear empties an account (other accounts untouched)', async () => {
    const store = new MemoryUndoHistoryStore()
    await store.pushForward('acc1', step('a'))
    await store.pushForward('acc2', step('b', 'acc2'))
    await store.clear('acc1')
    expect(store.snapshot('acc1').steps).toEqual([])
    expect(store.snapshot('acc2').steps.map((s) => s.id)).toEqual(['b'])
  })

  it('persists across reload (shared backing) — per-account snapshots survive', async () => {
    const backing = new Map<string, UndoHistorySnapshot>()
    const store1 = new MemoryUndoHistoryStore(backing)
    await store1.pushForward('acc1', step('a', 'acc1', 1))
    await store1.pushForward('acc1', step('b', 'acc1', 2))
    await store1.undo() // cursor at a
    // A fresh store on the same backing reloads the persisted per-account state.
    const store2 = new MemoryUndoHistoryStore(backing)
    await store2.load()
    expect(store2.snapshot('acc1').steps.map((s) => s.id)).toEqual(['a', 'b'])
    expect(store2.snapshot('acc1').cursor).toBe(0) // cursor preserved across reload
  })
})

describe('undo history store — multi-account global merge', () => {
  it('undo targets the latest step across accounts (by createdAt)', async () => {
    const store = new MemoryUndoHistoryStore()
    await store.pushForward('acc1', step('a', 'acc1', 1))
    await store.pushForward('acc2', step('b', 'acc2', 5))
    await store.pushForward('acc1', step('c', 'acc1', 3))
    // Global undo order by createdAt: b (5), c (3), a (1).
    expect((await store.undo())?.step.id).toBe('b')
    expect((await store.undo())?.step.id).toBe('c')
    expect((await store.undo())?.step.id).toBe('a')
    expect(await store.undo()).toBeNull()
  })

  it('canUndo/canRedo span all accounts', async () => {
    const store = new MemoryUndoHistoryStore()
    expect(store.canUndo()).toBe(false)
    await store.pushForward('acc1', step('a'))
    expect(store.canUndo()).toBe(true)
    expect(store.canRedo()).toBe(false)
    await store.undo()
    expect(store.canUndo()).toBe(false)
    expect(store.canRedo()).toBe(true)
    // A second account with an applied step keeps canUndo true.
    await store.pushForward('acc2', step('b', 'acc2', 2))
    expect(store.canUndo()).toBe(true)
  })

  it('redo targets the latest redoable step across accounts', async () => {
    const store = new MemoryUndoHistoryStore()
    await store.pushForward('acc1', step('a', 'acc1', 1))
    await store.pushForward('acc2', step('b', 'acc2', 5))
    await store.undo() // undo b (acc2)
    await store.undo() // undo a (acc1)
    // Redo order by createdAt: b (5), a (1).
    expect((await store.redo())?.step.id).toBe('b')
    expect((await store.redo())?.step.id).toBe('a')
    expect(await store.redo()).toBeNull()
  })

  it('accounts are isolated — undoing acc1 never touches acc2', async () => {
    const store = new MemoryUndoHistoryStore()
    await store.pushForward('acc1', step('a', 'acc1', 1))
    await store.pushForward('acc2', step('b', 'acc2', 2))
    await store.undo() // undo b (latest)
    expect(store.snapshot('acc1').cursor).toBe(0) // acc1 untouched
    expect(store.snapshot('acc2').cursor).toBe(-1) // acc2 undone
  })
})

describe('undo history store — Phase 2 mirror (reconcileWithServer)', () => {
  /** Build a server `RevLogSnapshotWire` for an account from steps (seq = index+1). */
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
        diff: DIFF,
        createdAt: `2026-01-0${i + 1}T00:00:00Z`,
      })),
      cursor: { cursorStepId, redoTail },
    }
  }

  it('adopts an empty server snapshot (no pending move → clears local state)', async () => {
    const backing = new Map([
      ['acc1', { steps: [step('a')], cursor: 0 }] as [
        string,
        UndoHistorySnapshot,
      ],
    ])
    const store = new MemoryUndoHistoryStore(backing)
    await store.load()
    await store.reconcileWithServer('acc1', serverSnapshot([], null))
    expect(store.snapshot('acc1').steps).toEqual([])
    expect(store.snapshot('acc1').cursor).toBe(-1)
  })

  it('adopts the server steps + cursor (cross-device convergence)', async () => {
    const store = new MemoryUndoHistoryStore()
    await store.reconcileWithServer('acc1', serverSnapshot(['a', 'b'], 'b'))
    const snap = store.snapshot('acc1')
    expect(snap.steps.map((s) => s.id)).toEqual(['a', 'b'])
    expect(snap.cursor).toBe(1) // b
  })

  it('derives cursor = -1 when the server cursor is null (all undone)', async () => {
    const store = new MemoryUndoHistoryStore()
    await store.reconcileWithServer(
      'acc1',
      serverSnapshot(['a', 'b'], null, ['a', 'b']),
    )
    expect(store.snapshot('acc1').cursor).toBe(-1)
  })

  it('skips adoption while a local move is in-flight (optimism guard)', async () => {
    const store = new MemoryUndoHistoryStore()
    await store.pushForward('acc1', step('a')) // pendingConfirm = {a}
    // A STALE server snapshot arrives (the server hasn't confirmed the append).
    await store.reconcileWithServer('acc1', serverSnapshot([], null))
    const snap = store.snapshot('acc1')
    expect(snap.steps.map((s) => s.id)).toEqual(['a']) // preserved
    expect(snap.cursor).toBe(0) // not reverted to -1
  })

  it('skips a stale cursor after undo (does not revert the optimistic undo)', async () => {
    const store = new MemoryUndoHistoryStore()
    await store.pushForward('acc1', step('a'))
    await store.undo() // cursor → -1; pendingConfirm = {null}
    await store.reconcileWithServer('acc1', serverSnapshot(['a'], 'a'))
    expect(store.snapshot('acc1').cursor).toBe(-1) // optimistic undo preserved
  })

  it('confirms when the server echoes the optimistic cursor', async () => {
    const store = new MemoryUndoHistoryStore()
    await store.pushForward('acc1', step('a')) // pendingConfirm = {a}
    await store.reconcileWithServer('acc1', serverSnapshot([], null)) // stale → skip
    expect(store.snapshot('acc1').cursor).toBe(0)
    await store.reconcileWithServer('acc1', serverSnapshot(['a'], 'a')) // confirm
    expect(store.snapshot('acc1').cursor).toBe(0)
    // pendingConfirm cleared: a divergent server cursor is now adopted.
    await store.reconcileWithServer('acc1', serverSnapshot(['a', 'b'], 'b'))
    expect(store.snapshot('acc1').cursor).toBe(1) // adopted b
  })

  it('converges to the server after the optimism timeout (lost revCursor)', async () => {
    const store = new MemoryUndoHistoryStore()
    const restore = _optimismConfigForTesting.timeoutMs
    _optimismConfigForTesting.timeoutMs = 0 // expire immediately
    try {
      await store.pushForward('acc1', step('a')) // pendingConfirm = {a, now}
      await store.reconcileWithServer('acc1', serverSnapshot([], null))
      expect(store.snapshot('acc1').cursor).toBe(-1) // converged to server
    } finally {
      _optimismConfigForTesting.timeoutMs = restore
    }
  })
})
