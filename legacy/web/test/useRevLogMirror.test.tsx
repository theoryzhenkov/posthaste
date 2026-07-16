// Phase 2 Slice 5b-2: the `useRevLogMirror` hook subscribes to the per-account
// `RevLog` synced view + reconciles the client's undo/redo history store with
// the server-authoritative log (the RECEIVE half of cross-device cursor sync).
//
// spec: docs/eph/DESIGN-L2-undo-redo-revlog-contract
import { afterEach, beforeEach, describe, expect, it } from 'bun:test'
import type { ReactNode } from 'react'
import { renderHook, waitFor } from '@testing-library/react'

import { useRevLogMirror } from '../src/hooks/useRevLogMirror'
import {
  resetUndoHistoryStoreForTesting,
  setUndoHistoryStoreForTesting,
  MemoryUndoHistoryStore,
  type RevLogSnapshotWire,
} from '../src/runtime/replica/undoHistoryStore'
import {
  createFakeRuntimeAdapter,
  type FakeRuntimeAdapter,
} from '../src/runtime/fakeAdapter'
import {
  resetRuntimeAdapterForTesting,
  setRuntimeAdapterForTesting,
} from '../src/runtime/adapter'
import { resetRuntimeLinkClientForTesting } from '../src/runtime/linkClient'
import type { RuntimeViewSnapshot } from '../src/runtime/types'
import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

function revLogSnapshot(
  stepIds: string[],
  cursorStepId: string | null,
  redoTail: string[] = [],
): RevLogSnapshotWire {
  return {
    steps: stepIds.map((id, i) => ({
      stepId: id,
      seq: i + 1,
      messageId: 'm1',
      sourceId: 'primary',
      diff: {
        keywords: { added: [], removed: [] },
        mailboxes: { added: ['x'], removed: [] },
      },
      createdAt: `2026-01-0${i + 1}T00:00:00Z`,
    })),
    cursor: { cursorStepId, redoTail },
  }
}

function viewSnapshot<TData>(
  viewId: string,
  data: TData,
): RuntimeViewSnapshot<TData> {
  return {
    viewId,
    descriptor: { family: 'revLog', payload: {} },
    revision: 1,
    lifecycle: 'ready',
    readWatermark: null,
    coverage: { kind: 'complete' },
    data,
    pendingMutations: [],
    error: null,
  }
}

let runtimeAdapter: FakeRuntimeAdapter
let store: MemoryUndoHistoryStore

beforeEach(() => {
  runtimeAdapter = createFakeRuntimeAdapter()
  setRuntimeAdapterForTesting(runtimeAdapter)
  store = new MemoryUndoHistoryStore()
  setUndoHistoryStoreForTesting(store)
  runtimeAdapter.queueRuntimeLinkConnection({ linkId: 'link-1' })
})

afterEach(() => {
  resetRuntimeLinkClientForTesting()
  resetRuntimeAdapterForTesting()
  resetUndoHistoryStoreForTesting()
})

describe('useRevLogMirror (Phase 2 RevLog view → store reconciliation)', () => {
  it('opens the revLog view + adopts the initial snapshot', async () => {
    runtimeAdapter.queueRuntimeLinkView({
      viewId: 'view-1',
      snapshot: viewSnapshot('view-1', revLogSnapshot(['a', 'b'], 'b')),
    })

    renderHook(() => useRevLogMirror('primary'), {
      wrapper: ({ children }: { children: ReactNode }) => children as ReactNode,
    })

    await waitFor(() =>
      expect(store.snapshot('primary').steps.map((s) => s.id)).toEqual([
        'a',
        'b',
      ]),
    )
    expect(store.snapshot('primary').cursor).toBe(1) // b

    // The view was opened with family 'revLog' + the accountId payload.
    expect(runtimeAdapter.runtimeLinkObjectViewOpenCalls).toHaveLength(1)
    expect(runtimeAdapter.runtimeLinkObjectViewOpenCalls[0].descriptor).toEqual(
      { family: 'revLog', payload: { accountId: 'primary' } },
    )
  })

  it('reconciles viewReplace frames (cross-device updates)', async () => {
    runtimeAdapter.queueRuntimeLinkView({
      viewId: 'view-1',
      snapshot: viewSnapshot('view-1', revLogSnapshot(['a'], 'a')),
    })

    renderHook(() => useRevLogMirror('primary'), {
      wrapper: ({ children }: { children: ReactNode }) => children as ReactNode,
    })

    await waitFor(() =>
      expect(store.snapshot('primary').steps.map((s) => s.id)).toEqual(['a']),
    )

    // Another device appended step 'b' + the cursor advanced to it.
    runtimeAdapter.emitRuntimeFrame({
      type: 'viewReplace',
      linkSeq: 2,
      viewId: 'view-1',
      revision: 2,
      snapshot: viewSnapshot('view-1', revLogSnapshot(['a', 'b'], 'b')),
    })

    await waitFor(() =>
      expect(store.snapshot('primary').steps.map((s) => s.id)).toEqual([
        'a',
        'b',
      ]),
    )
    expect(store.snapshot('primary').cursor).toBe(1) // b
  })

  it('does nothing when accountId is null', async () => {
    renderHook(() => useRevLogMirror(null), {
      wrapper: ({ children }: { children: ReactNode }) => children as ReactNode,
    })
    // No view opened; the store stays empty.
    expect(runtimeAdapter.runtimeLinkObjectViewOpenCalls).toHaveLength(0)
  })
})
