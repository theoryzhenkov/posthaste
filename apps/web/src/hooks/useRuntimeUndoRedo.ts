/**
 * Renderer access to the runtime-owned undo/redo history.
 *
 * The runtime computes and broadcasts the current top of each stack via
 * `mutationHistory` frames (`undoTop` / `redoTop`). This hook mirrors the
 * availability bits and constructs ordinary optimistic `message.applyDiff`
 * mutations to execute undo/redo, so they flow through the same local-first
 * guard as any user action.
 *
 * @spec docs/runtime/mutations/L1#mutation-pipeline-and-catalog
 */
import { useCallback, useEffect, useState } from 'react'

import { undoLogger } from '@/logger'
import type { DiffStep } from '@/runtime/replica/handle'
import { invertMessageChangeDiff } from '@/runtime/replica/handle'
import { runtimeSessionClient } from '@/runtime/sessionClient'

export interface RuntimeUndoRedo {
  canUndo: boolean
  canRedo: boolean
  undo: () => void
  redo: () => void
}

export function useRuntimeUndoRedo(): RuntimeUndoRedo {
  const [undoTop, setUndoTop] = useState<DiffStep | null>(null)
  const [redoTop, setRedoTop] = useState<DiffStep | null>(null)

  useEffect(() => {
    const unsubscribe = runtimeSessionClient.subscribe(
      {
        onFrame(frame) {
          if (frame.type === 'mutationHistory') {
            undoLogger.debug(
              {
                sessionSeq: frame.sessionSeq,
                canUndo: frame.canUndo,
                canRedo: frame.canRedo,
                undoTopSeq: frame.undoTop?.seq,
                redoTopSeq: frame.redoTop?.seq,
              },
              'mutationHistory frame updated undo/redo tops',
            )
            setUndoTop(frame.undoTop ?? null)
            setRedoTop(frame.redoTop ?? null)
          }
        },
      },
      { afterSeq: 0 },
    )
    return unsubscribe
  }, [])

  const runApplyDiff = useCallback((step: DiffStep, inverse: boolean) => {
    void runtimeSessionClient
      .runMutation({
        name: 'message.applyDiff',
        args: {
          sourceId: step.sourceId,
          messageId: step.messageId,
          diff: inverse ? invertMessageChangeDiff(step.diff) : step.diff,
          [inverse ? 'undoOf' : 'redoOf']: step.seq,
        },
      })
      .catch(() => {
        // Transient failures are non-fatal; availability is corrected by the
        // next mutationHistory frame.
      })
  }, [])

  const undo = useCallback(() => {
    const step = undoTop
    undoLogger.debug(
      { currentUndoTopSeq: step?.seq, canUndo: step !== null },
      'undo requested',
    )
    if (!step) return
    runApplyDiff(step, true)
  }, [undoTop, runApplyDiff])

  const redo = useCallback(() => {
    const step = redoTop
    undoLogger.debug(
      { currentRedoTopSeq: step?.seq, canRedo: step !== null },
      'redo requested',
    )
    if (!step) return
    runApplyDiff(step, false)
  }, [redoTop, runApplyDiff])

  return {
    canRedo: redoTop !== null,
    canUndo: undoTop !== null,
    redo,
    undo,
  }
}
