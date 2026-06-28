import { useEffect } from 'react'

import { runtimeSessionClient } from '@/runtime/sessionClient'
import type { RuntimeViewSnapshot } from '@/runtime/types'
import {
  getUndoHistoryStore,
  type RevLogSnapshotWire,
} from '@/runtime/replica/undoHistoryStore'

/**
 * Phase 2: subscribe to the per-account `RevLog` synced view + reconcile the
 * client's undo/redo history store with the server-authoritative log. This is
 * the RECEIVE half of cross-device cursor sync (Slice 5a sent the `revCursor`):
 * the store adopts the server's steps + cursor so undo on one device reflects
 * forward actions + undo/redo state from other devices.
 *
 * The store's `reconcileWithServer` applies the optimism guard — a local
 * undo/redo/forward move stays optimistic (the `revCursor`/append round-trip
 * is fast); the mirror only adopts a server cursor that confirms the move (or
 * converges after a timeout if the `revCursor` was lost/overridden). So the
 * round-trip-free UX from Phase 1 is preserved.
 *
 * Degrades gracefully: if the view can't open (no session, transport error),
 * the store keeps its local (Phase 1) state — undo/redo still works locally.
 *
 * @spec docs/eph/DESIGN-L2-undo-redo-revlog-contract
 */
export function useRevLogMirror(accountId: string | null): void {
  useEffect(() => {
    if (!accountId) {
      return
    }

    let closed = false
    let viewId: string | undefined
    let unsubscribe: (() => void) | undefined
    const store = getUndoHistoryStore()

    const reconcile = (data: RevLogSnapshotWire | undefined): void => {
      if (data) {
        void store.reconcileWithServer(accountId, data)
      }
    }

    void runtimeSessionClient
      .openView<RevLogSnapshotWire>({
        family: 'revLog',
        payload: { accountId },
        sourceId: accountId,
      })
      .then((opened) => {
        viewId = opened.viewId
        if (closed) {
          const closingViewId = viewId
          viewId = undefined
          runtimeSessionClient.closeView(closingViewId)
          return
        }
        const openedViewId = opened.viewId
        reconcile(opened.snapshot.data)
        unsubscribe = runtimeSessionClient.subscribe(
          {
            onFrame(frame) {
              switch (frame.type) {
                case 'viewSnapshot':
                case 'viewReplace':
                  if (frame.viewId !== openedViewId) {
                    return
                  }
                  reconcile(
                    (frame.snapshot as RuntimeViewSnapshot<RevLogSnapshotWire>)
                      .data,
                  )
                  return
                case 'viewError':
                case 'viewClosed':
                case 'mutationNotification':
                case 'notification':
                case 'heartbeat':
                  return
              }
            },
          },
          { afterSeq: 0, sourceId: accountId },
        )
      })
      .catch(() => {
        // Degrade to local-only history (Phase 1). The store keeps its local
        // state; undo/redo still works without cross-device sync.
      })

    return () => {
      closed = true
      unsubscribe?.()
      if (viewId) {
        runtimeSessionClient.closeView(viewId)
      }
    }
  }, [accountId])
}
