/**
 * Undo/redo over the backend rev-log. The `revLog` family serves the
 * account's reversible-operation log with its cursor; the `undo`/`redo`
 * commands move the cursor server-side, and the answers (list rows, counts,
 * the log itself) catch up through the global invalidation.
 *
 * Per-account: pass the account whose history the shell's Ctrl+Z should
 * drive. With no account (no enabled accounts yet) the verbs are inert.
 */
import { useCallback, useMemo } from 'react'
import { toast } from 'sonner'

import { useCommands, useRevLog } from '@/data'

export interface UndoRedo {
  canUndo: boolean
  canRedo: boolean
  undo: () => void
  redo: () => void
}

export function useUndoRedo(accountId?: string | null): UndoRedo {
  const commands = useCommands()
  const revLog = useRevLog(
    { accountId: accountId ?? '' },
    { enabled: accountId != null },
  )

  const cursor = revLog.data?.cursor
  const canUndo = Boolean(accountId && cursor?.cursorStepId)
  const canRedo = Boolean(accountId && cursor && cursor.redoTail.length > 0)

  const undo = useCallback(() => {
    if (!accountId) {
      return
    }
    void commands.undo(accountId).catch((error: unknown) => {
      toast.error(
        error instanceof Error ? error.message : 'Nothing to undo',
      )
    })
  }, [accountId, commands])

  const redo = useCallback(() => {
    if (!accountId) {
      return
    }
    void commands.redo(accountId).catch((error: unknown) => {
      toast.error(
        error instanceof Error ? error.message : 'Nothing to redo',
      )
    })
  }, [accountId, commands])

  return useMemo(
    () => ({ canUndo, canRedo, undo, redo }),
    [canUndo, canRedo, undo, redo],
  )
}
