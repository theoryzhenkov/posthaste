/**
 * Client-owned undo/redo — Phase 1 of the synced-history refactor. The history
 * lives in the (durable) `undoHistoryStore`; this hook reads its cursor and
 * dispatches a plain `message.applyDiff` for each undo/redo. Navigation is
 * LOCAL: there is no `busyRef` serialization, so rapid undos all dispatch
 * immediately — N undos do not cost N runtime round trips. The view update is
 * optimistic (the adapter folds the applyDiff); the cursor moves in memory
 * before the dispatch settles.
 *
 * @spec docs/eph/DESIGN-L2-undo-redo-synced-history
 */
import { useEffect, useState } from 'react'

import {
  getUndoHistoryStore,
  type UndoHistorySnapshot,
} from '@/runtime/replica/undoHistoryStore'
import { runtimeSessionClient } from '@/runtime/sessionClient'
import { invertMessageChangeDiff } from '@/runtime/replica/wasmUtil'

export interface UndoRedo {
  canUndo: boolean
  canRedo: boolean
  undo: () => void
  redo: () => void
}

export function useUndoRedo(): UndoRedo {
  const store = getUndoHistoryStore()
  const [snap, setSnap] = useState<UndoHistorySnapshot>(() => store.snapshot())

  useEffect(() => {
    let active = true
    void store.load().then((s) => {
      if (active) setSnap(s)
    })
    const unsubscribe = store.subscribe((s) => setSnap(s))
    return () => {
      active = false
      unsubscribe()
    }
  }, [store])

  const canUndo = snap.cursor >= 0
  const canRedo = snap.cursor < snap.steps.length - 1

  const undo = () => {
    void store.navigateUndo().then(async (step) => {
      if (!step) return
      const inverse = await invertMessageChangeDiff(step.diff)
      void runtimeSessionClient
        .runMutation({
          name: 'message.applyDiff',
          args: {
            sourceId: step.sourceId,
            messageId: step.messageId,
            diff: inverse,
          },
        })
        .catch(() => {
          // Transient transport failures are non-fatal; the outbox/convergence
          // guard reconciles. (Cursor rollback on a rejected settlement is the
          // Phase 2 concern; the applyDiff is idempotent.)
        })
    })
  }

  const redo = () => {
    void store.navigateRedo().then((step) => {
      if (!step) return
      void runtimeSessionClient
        .runMutation({
          name: 'message.applyDiff',
          args: {
            sourceId: step.sourceId,
            messageId: step.messageId,
            diff: step.diff,
          },
        })
        .catch(() => {})
    })
  }

  return { canUndo, canRedo, undo, redo }
}
