/**
 * Consumer hooks for the mail keyboard controller: read/move pane focus and
 * register a pane's focused-key handler.
 *
 */
import { useEffect, useRef } from 'react'

import { useKeyboardContext } from './context'
import type { PaneId } from '@/domain/vocabulary'

import type { PaneKeyHandler } from './dispatch'

/** Which pane currently owns within-pane keys, plus a setter for click-to-focus. */
export function useActivePane(): {
  activePane: PaneId
  focusPane: (pane: PaneId) => void
} {
  const { activePane, focusPane } = useKeyboardContext()
  return { activePane, focusPane }
}

/**
 * Register `handler` as the focused-key handler for `pane`. The latest closure
 * is always invoked, so callers need not memoize `handler`.
 */
export function useFocusedPaneHandler(
  pane: PaneId,
  handler: PaneKeyHandler,
): void {
  const { registerPaneHandler } = useKeyboardContext()
  const handlerRef = useRef(handler)
  useEffect(() => {
    handlerRef.current = handler
  })
  useEffect(
    () => registerPaneHandler(pane, (event) => handlerRef.current(event)),
    [pane, registerPaneHandler],
  )
}
