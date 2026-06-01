/**
 * {@link OperationsProvider}: owns the optimistic mail-operation runner and the
 * undo/redo history.
 *
 * Running an operation captures each target's mutable before-image, applies the
 * optimistic cache patch (keyword and/or mailbox membership), sends the derived
 * command(s), rolls back on error, and reconciles server truth on success.
 * Invertible operations are pushed onto the undo stack; undo/redo replay the
 * recorded before/after images through the same runner, so undo restores a
 * message to *where it actually was* rather than a hardcoded destination.
 *
 * `destroy` is irreversible: it runs optimistically but is never recorded, and
 * performing any fresh operation clears the redo stack.
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
import { performMessageCommand } from '../api/client'
import type { MessageCommand } from '../api/types'
import {
  invalidateMessageMutationReadModels,
  invalidateMessageScopeReadModels,
} from '../domainCache'
import {
  applyKeywordPatch,
  applyMailboxPatch,
  captureMutableState,
  deriveKeywordState,
  diffMutableState,
  findConversationIdForMessage,
  mergeMessageDetail,
  recordLocalMutationEvents,
  restoreSnapshots,
  type MailSelection,
  type MutableState,
  type QuerySnapshot,
} from '../mailState'
import {
  invertOperation,
  replayOperation,
  type AppliedOperation,
  type MailOperation,
  type OperationEntry,
  type OperationTarget,
} from '../operations'
import {
  OperationsContext,
  type OperationsContextValue,
} from '../operationsContext'

/** Maximum number of operations retained for undo/redo. */
const MAX_HISTORY = 50

/** Fresh before-image for an uncached target (never a shared reference). */
function neutralState(): MutableState {
  return { keywords: [], mailboxIds: [] }
}

/** Outcome of applying one operation to the cache + server. */
interface RunOutcome {
  /** Recorded operation when invertible; null for destroy / uncapturable. */
  applied: AppliedOperation | null
  ok: boolean
}

/**
 * Build the {@link MailSelection} for cache patching. List removal keys only on
 * source + message id; an absent conversation id (`''`) simply makes the
 * conversation-view patch a no-op (no cache entry matches), so we never block
 * the list update on a missing conversation.
 */
function selectionFor(target: OperationTarget): MailSelection {
  return {
    conversationId: target.conversationId ?? '',
    messageId: target.messageId,
    sourceId: target.sourceId,
  }
}

export function OperationsProvider({
  children,
}: {
  children: ReactNode
}): ReactNode {
  const queryClient = useQueryClient()
  const [errorMessage, setErrorMessage] = useState<string | null>(null)
  const pendingRef = useRef(0)
  const [isPending, setIsPending] = useState(false)
  // The stacks are authoritative in refs so undo/redo can pop atomically inside
  // the event handler (before any await), avoiding double-undo when the user
  // fires the action twice in quick succession. `counts` mirrors their lengths
  // into render state so canUndo/canRedo stay reactive without reading refs
  // during render.
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

  /**
   * Apply one operation to the cache + server. Handles optimism, rollback, and
   * reconciliation but not history/toasts (the caller manages those so undo and
   * redo can drive the same runner without recursive bookkeeping).
   */
  const runInternal = useCallback(
    async (operation: MailOperation): Promise<RunOutcome> => {
      const destroy = operation.kind === 'destroy'

      const prepared = operation.targets.map((rawTarget) => {
        const conversationId =
          rawTarget.conversationId ??
          findConversationIdForMessage(queryClient, rawTarget) ??
          undefined
        const target: OperationTarget = { ...rawTarget, conversationId }
        const captured = captureMutableState(queryClient, target)
        const before = captured ?? neutralState()
        return {
          after: destroy ? before : operation.project(target, before),
          before,
          captured: captured !== null,
          target,
        }
      })

      // Without a before-image we cannot faithfully restore, so the whole
      // operation becomes non-invertible (server reconciliation still corrects
      // the optimistic gaps via invalidation below).
      const invertible = !destroy && prepared.every((entry) => entry.captured)

      const snapshots: QuerySnapshot[] = []
      const recordedEntries: OperationEntry[] = []

      for (const { target, before, after, captured } of prepared) {
        if (captured) {
          recordedEntries.push({ after, before, target })
        }
        if (!captured) {
          continue
        }
        const selection = selectionFor(target)
        const commands = destroy
          ? [{ kind: 'destroy' } as MessageCommand]
          : diffMutableState(before, after)
        if (
          destroy ||
          commands.some((command) => command.kind === 'replaceMailboxes')
        ) {
          const result = applyMailboxPatch(
            queryClient,
            selection,
            after.mailboxIds,
            { destroy },
          )
          snapshots.push(...result.snapshots)
        }
        if (
          !destroy &&
          commands.some((command) => command.kind === 'setKeywords')
        ) {
          const result = applyKeywordPatch(queryClient, selection, {
            next: deriveKeywordState(after.keywords),
            previous: deriveKeywordState(before.keywords),
          })
          snapshots.push(...result.snapshots)
        }
      }

      setPending(1)
      try {
        for (const { target, before, after } of prepared) {
          const commands = destroy
            ? [{ kind: 'destroy' } as MessageCommand]
            : diffMutableState(before, after)
          for (const command of commands) {
            const result = await performMessageCommand(
              target.messageId,
              command,
              target.sourceId,
            )
            recordLocalMutationEvents(result.events)
            if (!destroy && result.detail && target.conversationId) {
              mergeMessageDetail(
                queryClient,
                result.detail,
                target.conversationId,
              )
            }
          }
          void invalidateMessageMutationReadModels(queryClient, target)
          void invalidateMessageScopeReadModels(
            queryClient,
            target,
            target.conversationId ?? null,
          )
        }
      } catch (error) {
        if (snapshots.length) {
          restoreSnapshots(queryClient, snapshots)
        }
        // Snapshots are whole-query images, so a rollback can resurrect rows a
        // concurrent op had already removed. Invalidate the affected read
        // models so the list re-syncs to server truth regardless.
        for (const { target } of prepared) {
          void invalidateMessageMutationReadModels(queryClient, target)
        }
        setErrorMessage(
          error instanceof Error ? error.message : 'Operation failed',
        )
        return { applied: null, ok: false }
      } finally {
        setPending(-1)
      }

      const applied: AppliedOperation | null = invertible
        ? {
            entries: recordedEntries,
            invertible: true,
            kind: operation.kind,
            label: operation.label,
            undoLabel: operation.undoLabel,
          }
        : null
      return { applied, ok: true }
    },
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
        if (!ok) {
          return
        }
        // Keyword toggles (read/flag/tags, including auto-mark-read) run
        // optimistically but are intentionally not recorded or toasted: they
        // are trivially reversed by toggling again, and auto actions must never
        // sit on the undo stack. Moves and deletes are the recorded actions.
        if (operation.kind === 'keywords') {
          return
        }
        // A fresh recorded action invalidates the redo timeline.
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
    // Pop synchronously so two quick Cmd+Z presses revert distinct entries.
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
        // Restore the stack move so the failed undo can be retried.
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
