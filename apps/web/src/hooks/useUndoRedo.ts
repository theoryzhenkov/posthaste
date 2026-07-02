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
import { useEffect, useReducer } from 'react'

import {
  getUndoHistoryStore,
  type UndoHistorySnapshot,
} from '@/runtime/replica/undoHistoryStore'
import { runtimeLinkClient } from '@/runtime/linkClient'
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
  void runtimeLinkClient
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
  // The store notifies on any account's change; re-read canUndo/canRedo (global
  // merge across accounts) via a force-update.
  const [, forceUpdate] = useReducer((x: number) => x + 1, 0)

  useEffect(() => {
    let active = true
    void store.load().then(() => {
      if (active) forceUpdate()
    })
    const unsubscribe = store.subscribe(() => forceUpdate())
    return () => {
      active = false
      unsubscribe()
    }
  }, [store])

  const canUndo = store.canUndo()
  const canRedo = store.canRedo()

  const undo = () => {
    void store.undo().then(async (result) => {
      if (!result) return
      const { step, accountId } = result
      const inverse = await invertMessageChangeDiff(step.diff)
      void runtimeLinkClient
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
      sendRevCursor(store.snapshot(accountId), accountId)
    })
  }

  const redo = () => {
    void store.redo().then((result) => {
      if (!result) return
      const { step, accountId } = result
      void runtimeLinkClient
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
      sendRevCursor(store.snapshot(accountId), accountId)
    })
  }

  return { canUndo, canRedo, undo, redo }
}
