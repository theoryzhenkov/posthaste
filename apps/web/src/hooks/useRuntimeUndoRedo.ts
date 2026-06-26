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
import { useCallback, useEffect, useRef, useState } from 'react'

import { LOG_EVENTS, undoLogger } from '@/logger'
import type { DiffStep } from '@/runtime/replica/handle'
import { invertMessageChangeDiff } from '@/runtime/replica/wasmUtil'
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

  // Refs hold the latest tops so queue processing can read them without
  // waiting for a React re-render. They are updated synchronously in the
  // frame handler before the queue is drained.
  const undoTopRef = useRef<DiffStep | null>(null)
  const redoTopRef = useRef<DiffStep | null>(null)

  // Serialize undo/redo requests: a new request is dispatched only after the
  // runtime has acknowledged the previous one via a mutationHistory frame. This
  // prevents rapid keypresses from reading a stale undoTop/redoTop and applying
  // the same diff step twice.
  const pendingRef = useRef<Array<'undo' | 'redo'>>([])
  const busyRef = useRef(false)

  const runApplyDiff = useCallback(async (step: DiffStep, inverse: boolean) => {
    undoLogger.debug(
      {
        event: LOG_EVENTS.historyApplyDiffDispatched,
        stepSeq: step.seq,
        inverse,
        sourceId: step.sourceId,
        messageId: step.messageId,
      },
      'dispatching applyDiff for history step',
    )
    const diff = inverse ? await invertMessageChangeDiff(step.diff) : step.diff
    void runtimeSessionClient
      .runMutation({
        name: 'message.applyDiff',
        args: {
          sourceId: step.sourceId,
          messageId: step.messageId,
          diff,
          [inverse ? 'undoOf' : 'redoOf']: step.seq,
        },
      })
      .catch(() => {
        // Transient failures are non-fatal; availability is corrected by the
        // next mutationHistory frame.
      })
  }, [])

  // Named function expression so the recursive call is not flagged as a
  // use-before-declare by the immutability ESLint rule; the const is assigned
  // at render time, well before the callback is ever invoked.
  const processQueue = useCallback(
    function processQueue(): void {
      if (busyRef.current || pendingRef.current.length === 0) {
        return
      }
      const kind = pendingRef.current.shift()
      if (!kind) {
        return
      }
      const step = kind === 'undo' ? undoTopRef.current : redoTopRef.current
      if (!step) {
        undoLogger.debug(
          {
            event: LOG_EVENTS.historyNavigationDropped,
            kind,
            reason: 'no step available',
          },
          'dropping queued history navigation',
        )
        processQueue()
        return
      }
      busyRef.current = true
      runApplyDiff(step, kind === 'undo')
    },
    [runApplyDiff],
  )

  useEffect(() => {
    const unsubscribe = runtimeSessionClient.subscribe(
      {
        onFrame(frame) {
          if (frame.type === 'mutationHistory') {
            undoLogger.debug(
              {
                event: LOG_EVENTS.runtimeFrameDispatched,
                sessionSeq: frame.sessionSeq,
                canUndo: frame.canUndo,
                canRedo: frame.canRedo,
                undoTopSeq: frame.undoTop?.seq,
                redoTopSeq: frame.redoTop?.seq,
              },
              'mutationHistory frame updated undo/redo tops',
            )
            undoTopRef.current = frame.undoTop ?? null
            redoTopRef.current = frame.redoTop ?? null
            setUndoTop(frame.undoTop ?? null)
            setRedoTop(frame.redoTop ?? null)
            if (busyRef.current) {
              busyRef.current = false
              processQueue()
            }
          }
        },
      },
      { afterSeq: 0 },
    )
    return unsubscribe
  }, [processQueue])

  const undo = useCallback(() => {
    undoLogger.debug(
      {
        event: LOG_EVENTS.historyUndoRequested,
        currentUndoTopSeq: undoTopRef.current?.seq,
        canUndo: undoTopRef.current !== null,
        queueLength: pendingRef.current.length,
        busy: busyRef.current,
      },
      'undo requested',
    )
    pendingRef.current.push('undo')
    processQueue()
  }, [processQueue])

  const redo = useCallback(() => {
    undoLogger.debug(
      {
        event: LOG_EVENTS.historyRedoRequested,
        currentRedoTopSeq: redoTopRef.current?.seq,
        canRedo: redoTopRef.current !== null,
        queueLength: pendingRef.current.length,
        busy: busyRef.current,
      },
      'redo requested',
    )
    pendingRef.current.push('redo')
    processQueue()
  }, [processQueue])

  return {
    canRedo: redoTop !== null,
    canUndo: undoTop !== null,
    redo,
    undo,
  }
}
