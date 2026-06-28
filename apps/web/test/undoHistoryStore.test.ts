// Client-owned undo/redo history — Phase 1. The store holds a `RevStep[]` +
// cursor, durable alongside the outbox, and navigation is LOCAL (no per-step
// round trip): chained undo pops the cursor in-memory and returns each step to
// invert. This is the round-trip-free win the design doc targets
// (@spec docs/eph/DESIGN-L2-undo-redo-synced-history#the-model).
import { describe, expect, it } from 'bun:test'

import {
  MemoryUndoHistoryStore,
  type RevStep,
  type UndoHistorySnapshot,
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
