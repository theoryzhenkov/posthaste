/**
 * Ambient data the type-directed condition editor needs to populate its value
 * pickers (mailbox / account). Supplied once at the editor root (via
 * `ConditionEditorContext.Provider`) so the recursive `RuleGroupEditor` →
 * `ConditionEditor` tree can render account-scoped pickers without threading
 * props through every level.
 *
 * Everything is optional: with the default (empty) context every widget still
 * renders and still emits the same wire value — pickers simply fall back to
 * showing the raw stored value, exactly like the previous text box.
 *
 */
import { createContext, useContext } from 'react'
import type { Mailbox } from '../../../api/types'

export interface ConditionEditorData {
  /** Account whose mailboxes the `mailboxId` picker should scope its query to. */
  accountId: string
  /**
   * Pre-fetched mailboxes for the picker, or null to let the picker query them
   * for `accountId` (matches the move-action picker's contract).
   */
  mailboxes: Mailbox[] | null
  /** Accounts offered by the `sourceId` account picker. */
  accounts: { id: string; name: string }[]
}

const EMPTY: ConditionEditorData = {
  accountId: '',
  mailboxes: null,
  accounts: [],
}

export const ConditionEditorContext = createContext<ConditionEditorData>(EMPTY)

export function useConditionEditorData(): ConditionEditorData {
  return useContext(ConditionEditorContext)
}
