/**
 * Renderer access to the runtime-owned undo/redo history.
 *
 * The runtime keeps the per-session undo/redo stack (one inverse per invertible
 * mutation) and broadcasts availability via `mutationHistory` frames; this hook
 * mirrors that into `canUndo`/`canRedo` and triggers `mutation.undo` /
 * `mutation.redo`. The renderer holds no history of its own.
 *
 * @spec docs/runtime/L2#mutation-pipeline-and-catalog
 */
import { useCallback, useEffect, useState } from 'react'

import { runtimeSessionClient } from '@/runtime/sessionClient'

export interface RuntimeUndoRedo {
  canUndo: boolean
  canRedo: boolean
  undo: () => void
  redo: () => void
}

export function useRuntimeUndoRedo(): RuntimeUndoRedo {
  const [availability, setAvailability] = useState({
    canRedo: false,
    canUndo: false,
  })

  useEffect(() => {
    const unsubscribe = runtimeSessionClient.subscribe(
      {
        onFrame(frame) {
          if (frame.type === 'mutationHistory') {
            setAvailability({
              canRedo: frame.canRedo,
              canUndo: frame.canUndo,
            })
          }
        },
      },
      { afterSeq: 0 },
    )
    return unsubscribe
  }, [])

  const undo = useCallback(() => {
    void runtimeSessionClient
      .runMutation({ name: 'mutation.undo', args: {} })
      .catch(() => {
        // Nothing-to-undo and transient failures are non-fatal; availability is
        // corrected by the next mutationHistory frame.
      })
  }, [])

  const redo = useCallback(() => {
    void runtimeSessionClient
      .runMutation({ name: 'mutation.redo', args: {} })
      .catch(() => {})
  }, [])

  return {
    canRedo: availability.canRedo,
    canUndo: availability.canUndo,
    redo,
    undo,
  }
}
