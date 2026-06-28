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

/**
 * Phase 2: send the server-arbitrated cursor assignment after a local undo/redo
 * move. The cursor is optimistic (the store moved it locally, instant UX); this
 * `revCursor` control mutation is the server arbitration that syncs the cursor
 * cross-device (last-writer-wins + idempotent — re-delivery/reorder safe). The
 * `cursorStepId`/`redoTail` are derived from the store's post-move snapshot.
 *
 * @spec docs/eph/DESIGN-L2-undo-redo-revlog-contract
 */
function sendRevCursor(snapshot: UndoHistorySnapshot, accountId: string): void {
  const cursorStepId =
    snapshot.cursor >= 0 ? (snapshot.steps[snapshot.cursor]?.id ?? null) : null
  const redoTail = snapshot.steps.slice(snapshot.cursor + 1).map((s) => s.id)
  void runtimeSessionClient
    .runMutation({
      name: 'revCursor',
      args: { accountId, cursorStepId, redoTail },
    })
    .catch(() => {
      // Transient transport failures are non-fatal; the outbox/convergence
      // guard reconciles. The local cursor is already correct (optimistic).
    })
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
      // Phase 2: arbitrate the cursor move with the server (cross-device sync).
      sendRevCursor(store.snapshot(), step.sourceId)
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
      // Phase 2: arbitrate the cursor move with the server (cross-device sync).
      sendRevCursor(store.snapshot(), step.sourceId)
    })
  }

  return { canUndo, canRedo, undo, redo }
}
