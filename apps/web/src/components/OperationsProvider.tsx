/**
 * {@link OperationsProvider}: owns the optimistic mail-operation runner and the
 * undo/redo history.
 *
 * @spec docs/L1-ui#undo-system
 * @spec docs/L1-ui#data-fetching
 */
import { useQueryClient } from '@tanstack/react-query'
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react'
import { toast } from 'sonner'
import {
  invertOperation,
  replayOperation,
  type AppliedOperation,
  type MailOperation,
} from '../operations'
import {
  OperationsContext,
  type OperationsContextValue,
} from '../operationsContext'
import { runOperationInternal } from './operations-provider/runInternal'

/** Maximum number of operations retained for undo/redo. */
const MAX_HISTORY = 50

export function OperationsProvider({
  children,
}: {
  children: ReactNode
}): ReactNode {
  const queryClient = useQueryClient()
  const [errorMessage, setErrorMessage] = useState<string | null>(null)
  const pendingRef = useRef(0)
  const [isPending, setIsPending] = useState(false)
  const undoStackRef = useRef<AppliedOperation[]>([])
  const redoStackRef = useRef<AppliedOperation[]>([])
  const [counts, setCounts] = useState({ redo: 0, undo: 0 })

  const syncCounts = useCallback(() => {
    setCounts({
      redo: redoStackRef.current.length,
      undo: undoStackRef.current.length,
    })
  }, [])

  const setPending = useCallback((delta: number) => {
    pendingRef.current = Math.max(0, pendingRef.current + delta)
    setIsPending(pendingRef.current > 0)
  }, [])

  const runInternal = useCallback(
    (operation: MailOperation) =>
      runOperationInternal({
        operation,
        queryClient,
        setErrorMessage,
        setPending,
      }),
    [queryClient, setPending],
  )

  // undo and redo reference each other (an undo toast offers redo and vice
  // versa). Refs break the lexical cycle; they are assigned in an effect and
  // only read inside event handlers, never during render.
  const undoRef = useRef<() => void>(() => {})
  const redoRef = useRef<() => void>(() => {})

  const run = useCallback(
    (operation: MailOperation) => {
      setErrorMessage(null)
      void (async () => {
        const { applied, ok } = await runInternal(operation)
        if (!ok || operation.kind === 'keywords') {
          return
        }
        redoStackRef.current = []
        if (applied) {
          undoStackRef.current = [...undoStackRef.current, applied].slice(
            -MAX_HISTORY,
          )
          toast(operation.label, {
            action: {
              label: operation.undoLabel ?? 'Undo',
              onClick: () => undoRef.current(),
            },
            duration: 5000,
          })
        } else {
          toast(operation.label, { duration: 5000 })
        }
        syncCounts()
      })()
    },
    [runInternal, syncCounts],
  )

  const undo = useCallback(() => {
    setErrorMessage(null)
    const applied = undoStackRef.current.at(-1)
    if (!applied) {
      return
    }
    const inverse = invertOperation(applied)
    if (!inverse) {
      return
    }
    undoStackRef.current = undoStackRef.current.slice(0, -1)
    redoStackRef.current = [...redoStackRef.current, applied].slice(
      -MAX_HISTORY,
    )
    syncCounts()
    void (async () => {
      const { ok } = await runInternal(inverse)
      if (!ok) {
        redoStackRef.current = redoStackRef.current.slice(0, -1)
        undoStackRef.current = [...undoStackRef.current, applied].slice(
          -MAX_HISTORY,
        )
        syncCounts()
        return
      }
      toast('Change reverted', {
        action: { label: 'Redo', onClick: () => redoRef.current() },
        duration: 5000,
      })
    })()
  }, [runInternal, syncCounts])

  const redo = useCallback(() => {
    setErrorMessage(null)
    const applied = redoStackRef.current.at(-1)
    if (!applied) {
      return
    }
    redoStackRef.current = redoStackRef.current.slice(0, -1)
    undoStackRef.current = [...undoStackRef.current, applied].slice(
      -MAX_HISTORY,
    )
    syncCounts()
    void (async () => {
      const { ok } = await runInternal(replayOperation(applied))
      if (!ok) {
        undoStackRef.current = undoStackRef.current.slice(0, -1)
        redoStackRef.current = [...redoStackRef.current, applied].slice(
          -MAX_HISTORY,
        )
        syncCounts()
        return
      }
      toast(applied.label, {
        action: { label: 'Undo', onClick: () => undoRef.current() },
        duration: 5000,
      })
    })()
  }, [runInternal, syncCounts])

  useEffect(() => {
    undoRef.current = undo
    redoRef.current = redo
  }, [undo, redo])

  const value = useMemo<OperationsContextValue>(
    () => ({
      canRedo: counts.redo > 0,
      canUndo: counts.undo > 0,
      clearError: () => setErrorMessage(null),
      errorMessage,
      isPending,
      redo,
      run,
      undo,
    }),
    [errorMessage, isPending, redo, run, undo, counts.redo, counts.undo],
  )

  return (
    <OperationsContext.Provider value={value}>
      {children}
    </OperationsContext.Provider>
  )
}
