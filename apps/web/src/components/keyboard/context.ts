/**
 * Shared context for the mail keyboard controller. Kept apart from the provider
 * component so the consumer hooks can live in a component-free module.
 *
 * @spec docs/L0-ui#navigation-model
 */
import { createContext, useContext } from 'react'

import type { PaneId, PaneKeyHandler } from './dispatch'

export interface KeyboardContextValue {
  activePane: PaneId
  focusPane: (pane: PaneId) => void
  registerPaneHandler: (pane: PaneId, handler: PaneKeyHandler) => () => void
}

export const KeyboardContext = createContext<KeyboardContextValue | null>(null)

export function useKeyboardContext(): KeyboardContextValue {
  const value = useContext(KeyboardContext)
  if (!value) {
    throw new Error(
      'useKeyboardContext must be used within a KeyboardController',
    )
  }
  return value
}
