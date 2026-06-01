/**
 * React context + hook for the mail operation runner.
 *
 * The runner executes {@link MailOperation}s optimistically and owns the
 * undo/redo history. The provider component lives in
 * `./components/OperationsProvider`; this module holds the context value type,
 * the context object, and the `useOperations` hook so the provider file can
 * export only a component (keeps fast-refresh happy).
 *
 * @spec docs/L1-ui#undo-system
 */
import { createContext, useContext } from 'react'
import type { MailOperation } from './operations'

/** Surface API exposed to components for running and undoing operations. */
export interface OperationsContextValue {
  /** Run an operation optimistically; pushes invertible ops onto the undo stack. */
  run: (operation: MailOperation) => void
  /** Revert the most recent invertible operation. */
  undo: () => void
  /** Re-apply the most recently undone operation. */
  redo: () => void
  canUndo: boolean
  canRedo: boolean
  isPending: boolean
  errorMessage: string | null
  clearError: () => void
}

export const OperationsContext = createContext<OperationsContextValue | null>(
  null,
)

/** Access the operation runner. Throws if used outside {@link OperationsProvider}. */
export function useOperations(): OperationsContextValue {
  const value = useContext(OperationsContext)
  if (!value) {
    throw new Error('useOperations must be used within an OperationsProvider')
  }
  return value
}
