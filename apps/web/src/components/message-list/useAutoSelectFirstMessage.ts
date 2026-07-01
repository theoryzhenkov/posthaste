import { useCallback, useEffect, useRef } from 'react'

import type { MessageSummary } from '../../api/types'

/**
 * Inputs for {@link useAutoSelectFirstMessage}.
 *
 * The hook is agnostic to how a row maps to a selection — callers pass a
 * `selectFirst` callback that performs that mapping — so it stays free of
 * selection-shape dependencies.
 */
export interface AutoSelectFirstMessageOptions {
  /** Whether the list pane currently owns keyboard focus. */
  isListActive: boolean
  /** Flattened rows in display order; the first is the auto-select anchor. */
  rows: ReadonlyArray<{ message: MessageSummary }>
  /** Key of the currently selected message, or null when nothing is selected. */
  selectedKey: string | null
  /** Stable key for the current view; changing it resets the skip flag. */
  currentViewKey: string
  /** Selects a message (opens it in the detail pane). */
  selectFirst: (message: MessageSummary) => void
  /** Clears the current selection (used by the skip-preserving clear). */
  clearSelection: () => void
}

/**
 * Keeps a highlighted "current" message whenever the list is the focused pane
 * and has rows but no selection, so focus and highlight can never diverge — no
 * "focused but not visible" gap (the message is implied by the nav anchor but
 * not rendered as selected until the user presses `j`/`k`).
 *
 * An explicit clear (background click to close the detail pane) sets a skip
 * flag so the detail pane stays closed until the user navigates or switches
 * views; switching views resets the flag (new context → re-anchor).
 *
 * Returns `clearAndSkip` for callers to wire to background-click clears so those
 * clears aren't immediately undone by the auto-select.
 *
 * @spec docs/ui/L0#navigation-model
 */
export function useAutoSelectFirstMessage({
  isListActive,
  rows,
  selectedKey,
  currentViewKey,
  selectFirst,
  clearSelection,
}: AutoSelectFirstMessageOptions) {
  const skipRef = useRef(false)
  const prevViewKeyRef = useRef(currentViewKey)

  // Background-click clear: set the skip flag so the next render's auto-select
  // effect doesn't immediately re-anchor (which would reopen the detail pane).
  const clearAndSkip = useCallback(() => {
    skipRef.current = true
    clearSelection()
  }, [clearSelection])

  useEffect(() => {
    // A view switch (mailbox change) is a new context: re-arm auto-select even
    // if the previous view was explicitly cleared.
    if (prevViewKeyRef.current !== currentViewKey) {
      skipRef.current = false
      prevViewKeyRef.current = currentViewKey
    }
    if (!isListActive || skipRef.current) return
    if (rows.length === 0 || selectedKey) return
    selectFirst(rows[0].message)
  }, [isListActive, rows, selectedKey, currentViewKey, selectFirst])

  return { clearAndSkip }
}
