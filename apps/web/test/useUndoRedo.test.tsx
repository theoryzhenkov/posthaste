// Phase 1 client-owned undo/redo hook. Navigation is LOCAL: the hook reads the
// history store's cursor + dispatches `message.applyDiff` for each undo/redo
// immediately — NO `busyRef` serialization, so rapid undos all dispatch without
// waiting for a runtime `mutationHistory` frame (the round-trip-free win).
import { afterEach, beforeEach, describe, expect, it } from 'bun:test'
import { act, renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'

import { useUndoRedo } from '../src/hooks/useUndoRedo'
import {
  resetRuntimeAdapterForTesting,
  setRuntimeAdapterForTesting,
} from '../src/runtime/adapter'
import {
  createFakeRuntimeAdapter,
  type FakeRuntimeAdapter,
} from '../src/runtime/fakeAdapter'
import {
  MemoryUndoHistoryStore,
  resetUndoHistoryStoreForTesting,
  setUndoHistoryStoreForTesting,
  type RevStep,
} from '../src/runtime/replica/undoHistoryStore'
import { resetRuntimeSessionClientForTesting } from '../src/runtime/sessionClient'
import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

function wrapper({ children }: { children: ReactNode }) {
  return <>{children}</>
}

let runtimeAdapter: FakeRuntimeAdapter
let store: MemoryUndoHistoryStore

beforeEach(() => {
  runtimeAdapter = createFakeRuntimeAdapter()
  setRuntimeAdapterForTesting(runtimeAdapter)
  store = new MemoryUndoHistoryStore()
  setUndoHistoryStoreForTesting(store)
})

afterEach(() => {
  resetRuntimeSessionClientForTesting()
  resetRuntimeAdapterForTesting()
  resetUndoHistoryStoreForTesting()
})

function makeStep(id: string, messageId = 'm1'): RevStep {
  return {
    id,
    messageId,
    sourceId: 'primary',
    diff: {
      keywords: { added: ['$flagged'], removed: [] },
      mailboxes: { added: [], removed: [] },
    },
    createdAt: Date.now(),
  }
}

function applyDiffArgs(call: {
  request: { name: string; args: Record<string, unknown> }
}): { diff: unknown } | null {
  if (call.request.name !== 'message.applyDiff') return null
  return call.request.args as { diff: unknown }
}

/** The `message.applyDiff` calls (filters out the Phase 2 `revCursor` calls). */
function applyDiffCalls(): {
  request: { name: string; args: Record<string, unknown> }
}[] {
  return runtimeAdapter.runtimeMutationCalls.filter(
    (c) => c.request.name === 'message.applyDiff',
  )
}

/** The `revCursor` control-mutation calls (Phase 2 cursor arbitration). */
function revCursorCalls(): {
  request: { name: string; args: Record<string, unknown> }
}[] {
  return runtimeAdapter.runtimeMutationCalls.filter(
    (c) => c.request.name === 'revCursor',
  )
}

describe('useUndoRedo (client-owned, round-trip-free)', () => {
  it('undo dispatches applyDiff with the inverse diff, with no frame round trip', async () => {
    runtimeAdapter.queueRuntimeSession({ sessionId: 'session-1' })
    runtimeAdapter.queueRuntimeMutationReceipt({
      runtimeMutationId: 'rm-1',
      clientMutationId: 'undo-1',
      name: 'message.applyDiff',
      state: 'confirmed',
      error: null,
      output: { events: [] },
    })
    await store.pushForward(makeStep('a'))

    const { result } = renderHook(() => useUndoRedo(), { wrapper })
    await waitFor(() => expect(result.current.canUndo).toBe(true))

    act(() => {
      result.current.undo()
    })
    await waitFor(() => expect(applyDiffCalls().length).toBe(1))
    const args = applyDiffArgs(applyDiffCalls()[0])
    expect(args).not.toBeNull()
    // inverse of {added:[$flagged],removed:[]} swaps → {added:[],removed:[$flagged]}
    expect(
      (args as { diff: { keywords: { added: string[]; removed: string[] } } })
        .diff.keywords,
    ).toEqual({
      added: [],
      removed: ['$flagged'],
    })
  })

  it('rapid undos all dispatch immediately (no busyRef serialization)', async () => {
    runtimeAdapter.queueRuntimeSession({ sessionId: 'session-1' })
    runtimeAdapter.queueRuntimeMutationReceipt({
      runtimeMutationId: 'rm-1',
      clientMutationId: 'u1',
      name: 'message.applyDiff',
      state: 'confirmed',
      error: null,
      output: { events: [] },
    })
    runtimeAdapter.queueRuntimeMutationReceipt({
      runtimeMutationId: 'rm-2',
      clientMutationId: 'u2',
      name: 'message.applyDiff',
      state: 'confirmed',
      error: null,
      output: { events: [] },
    })
    runtimeAdapter.queueRuntimeMutationReceipt({
      runtimeMutationId: 'rm-3',
      clientMutationId: 'u3',
      name: 'message.applyDiff',
      state: 'confirmed',
      error: null,
      output: { events: [] },
    })
    await store.pushForward(makeStep('a'))
    await store.pushForward(makeStep('b'))
    await store.pushForward(makeStep('c'))

    const { result } = renderHook(() => useUndoRedo(), { wrapper })
    await waitFor(() => expect(result.current.canUndo).toBe(true))

    // Three rapid undos — all dispatch without waiting for any ack frame.
    act(() => {
      result.current.undo()
      result.current.undo()
      result.current.undo()
    })
    await waitFor(() => expect(applyDiffCalls().length).toBe(3))
    // Order: c, b, a (newest undone first)
    const seq = applyDiffCalls().map(
      (c) =>
        (applyDiffArgs(c) as { diff: { keywords: { removed: string[] } } }).diff
          .keywords.removed,
    )
    expect(seq).toEqual([['$flagged'], ['$flagged'], ['$flagged']])
  })

  it('redo dispatches applyDiff with the forward diff', async () => {
    runtimeAdapter.queueRuntimeSession({ sessionId: 'session-1' })
    runtimeAdapter.queueRuntimeMutationReceipt({
      runtimeMutationId: 'rm-1',
      clientMutationId: 'redo-1',
      name: 'message.applyDiff',
      state: 'confirmed',
      error: null,
      output: { events: [] },
    })
    await store.pushForward(makeStep('a'))
    await store.navigateUndo()

    const { result } = renderHook(() => useUndoRedo(), { wrapper })
    await waitFor(() => expect(result.current.canRedo).toBe(true))

    act(() => {
      result.current.redo()
    })
    await waitFor(() => expect(applyDiffCalls().length).toBe(1))
    const args = applyDiffArgs(applyDiffCalls()[0])
    // forward diff (not inverted): added [$flagged]
    expect(
      (args as { diff: { keywords: { added: string[] } } }).diff.keywords.added,
    ).toEqual(['$flagged'])
  })

  it('undo sends a revCursor cursor-arbitration mutation (Phase 2)', async () => {
    runtimeAdapter.queueRuntimeSession({ sessionId: 'session-1' })
    runtimeAdapter.queueRuntimeMutationReceipt({
      runtimeMutationId: 'rm-1',
      clientMutationId: 'undo-1',
      name: 'message.applyDiff',
      state: 'confirmed',
      error: null,
      output: { events: [] },
    })
    await store.pushForward(makeStep('a'))

    const { result } = renderHook(() => useUndoRedo(), { wrapper })
    await waitFor(() => expect(result.current.canUndo).toBe(true))

    act(() => {
      result.current.undo()
    })
    await waitFor(() => expect(revCursorCalls().length).toBe(1))
    const revCursor = revCursorCalls()[0].request.args as {
      accountId: string
      cursorStepId: string | null
      redoTail: string[]
    }
    // After undoing step 'a', the cursor is all-undone (null) + 'a' is in the
    // redo tail. accountId = the step's sourceId.
    expect(revCursor.accountId).toBe('primary')
    expect(revCursor.cursorStepId).toBeNull()
    expect(revCursor.redoTail).toEqual(['a'])
  })

  it('canUndo/canRedo track the store cursor', async () => {
    runtimeAdapter.queueRuntimeSession({ sessionId: 'session-1' })
    const { result } = renderHook(() => useUndoRedo(), { wrapper })

    await act(() => store.pushForward(makeStep('a')))
    await waitFor(() => {
      expect(result.current.canUndo).toBe(true)
      expect(result.current.canRedo).toBe(false)
    })

    await act(() => store.navigateUndo())
    await waitFor(() => {
      expect(result.current.canUndo).toBe(false)
      expect(result.current.canRedo).toBe(true)
    })
  })

  it('a forward action clears the redo tail (no phantom redo)', async () => {
    runtimeAdapter.queueRuntimeSession({ sessionId: 'session-1' })
    const { result } = renderHook(() => useUndoRedo(), { wrapper })

    await act(() => store.pushForward(makeStep('a')))
    await act(() => store.pushForward(makeStep('b')))
    await act(() => store.navigateUndo()) // cursor at a; b redoable
    await waitFor(() => expect(result.current.canRedo).toBe(true))

    await act(() => store.pushForward(makeStep('c'))) // truncates b
    await waitFor(() => expect(result.current.canRedo).toBe(false))
    expect(result.current.canUndo).toBe(true)
  })
})
