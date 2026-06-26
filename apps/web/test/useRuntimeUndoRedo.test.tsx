import { afterEach, beforeEach, describe, expect, it } from 'bun:test'
import { renderHook, waitFor, act } from '@testing-library/react'
import type { ReactNode } from 'react'

import { useRuntimeUndoRedo } from '../src/hooks/useRuntimeUndoRedo'
import { queryKeys } from '../src/queryKeys'
import {
  resetRuntimeAdapterForTesting,
  setRuntimeAdapterForTesting,
} from '../src/runtime/adapter'
import {
  createFakeRuntimeAdapter,
  type FakeRuntimeAdapter,
} from '../src/runtime/fakeAdapter'
import { resetRuntimeSessionClientForTesting } from '../src/runtime/sessionClient'
import type {
  DiffStep,
  RuntimeFrame,
  RuntimeMutationReceipt,
} from '../src/runtime/types'
import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

let runtimeAdapter: FakeRuntimeAdapter

function wrapper({ children }: { children: ReactNode }) {
  return <>{children}</>
}

beforeEach(() => {
  runtimeAdapter = createFakeRuntimeAdapter()
  setRuntimeAdapterForTesting(runtimeAdapter)
})

afterEach(() => {
  resetRuntimeSessionClientForTesting()
  resetRuntimeAdapterForTesting()
})

function makeHistoryFrame(
  sessionSeq: number,
  undoTop: DiffStep | null = null,
  redoTop: DiffStep | null = null,
): RuntimeFrame {
  return {
    type: 'mutationHistory',
    sessionSeq,
    canUndo: undoTop !== null,
    canRedo: redoTop !== null,
    undoTop,
    redoTop,
  }
}

function makeUndoStep(seq: number): DiffStep {
  return {
    seq,
    sourceId: 'primary',
    messageId: 'm1',
    diff: {
      keywords: { added: ['$flagged'], removed: [] },
      mailboxes: { added: [], removed: [] },
    },
  }
}

function makeMutationReceipt(
  clientMutationId: string,
): RuntimeMutationReceipt {
  return {
    runtimeMutationId: 'mutation-1',
    clientMutationId,
    name: 'message.applyDiff',
    state: 'confirmed',
    error: null,
    output: { events: [] },
  }
}

describe('useRuntimeUndoRedo', () => {
  it('serializes rapid undos so only one is dispatched before the history frame updates', async () => {
    runtimeAdapter.queueRuntimeSession({ sessionId: 'session-1' })
    runtimeAdapter.queueRuntimeMutationReceipt(makeMutationReceipt('undo-1'))
    runtimeAdapter.queueRuntimeMutationReceipt(makeMutationReceipt('undo-2'))

    const { result } = renderHook(() => useRuntimeUndoRedo(), { wrapper })

    await waitFor(() =>
      expect(runtimeAdapter.runtimeFrameSubscriptionCalls.length).toBe(1),
    )

    act(() => {
      runtimeAdapter.emitRuntimeFrame(makeHistoryFrame(1, makeUndoStep(42)))
    })
    await waitFor(() => expect(result.current.canUndo).toBe(true))

    // Two rapid undo presses before a fresh mutationHistory frame arrives.
    act(() => {
      result.current.undo()
      result.current.undo()
    })

    await waitFor(() =>
      expect(runtimeAdapter.runtimeMutationCalls.length).toBe(1),
    )

    const onlyUndoOf = (
      runtimeAdapter.runtimeMutationCalls[0].request.args as {
        undoOf?: number
      }
    ).undoOf
    expect(onlyUndoOf).toBe(42)
  })

  it('dispatches the next queued undo after the mutationHistory frame arrives', async () => {
    runtimeAdapter.queueRuntimeSession({ sessionId: 'session-1' })
    runtimeAdapter.queueRuntimeMutationReceipt(makeMutationReceipt('undo-a'))
    runtimeAdapter.queueRuntimeMutationReceipt(makeMutationReceipt('undo-b'))

    const { result } = renderHook(() => useRuntimeUndoRedo(), { wrapper })

    await waitFor(() =>
      expect(runtimeAdapter.runtimeFrameSubscriptionCalls.length).toBe(1),
    )

    act(() => {
      runtimeAdapter.emitRuntimeFrame(makeHistoryFrame(1, makeUndoStep(42)))
    })
    await waitFor(() => expect(result.current.canUndo).toBe(true))

    act(() => {
      result.current.undo()
      result.current.undo()
    })

    await waitFor(() =>
      expect(runtimeAdapter.runtimeMutationCalls.length).toBe(1),
    )
    expect(
      (runtimeAdapter.runtimeMutationCalls[0].request.args as { undoOf?: number })
        .undoOf,
    ).toBe(42)

    // Runtime ack: the first undo moved step 42 to redo; the next undoable step is 41.
    act(() => {
      runtimeAdapter.emitRuntimeFrame(makeHistoryFrame(2, makeUndoStep(41)))
    })

    await waitFor(() =>
      expect(runtimeAdapter.runtimeMutationCalls.length).toBe(2),
    )
    expect(
      (runtimeAdapter.runtimeMutationCalls[1].request.args as { undoOf?: number })
        .undoOf,
    ).toBe(41)
  })
})
