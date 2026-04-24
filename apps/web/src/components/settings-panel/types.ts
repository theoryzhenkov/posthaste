/**
 * Shared types for the settings panel editor components.
 * @spec docs/L1-api#account-crud-lifecycle
 * @spec docs/L1-api#smart-mailbox-crud
 */
import type { SmartMailboxRule } from '../../api/types'

/** Editor target: `"new"` for create mode, or an existing entity ID. */
export type EditorTarget = 'new' | string
/** Smart mailbox editor target: `"new"` for create mode, or an existing mailbox ID. */
export type SmartMailboxEditorTarget = 'new' | string
/** @spec docs/L1-api#account-crud-lifecycle */
export interface AccountFormState {
  name: string
  fullName: string
  emailPatternsText: string
  baseUrl: string
  username: string
  password: string
}

/** @spec docs/L1-api#smart-mailbox-crud */
export interface SmartMailboxFormState {
  name: string
  position: number
  rule: SmartMailboxRule
}
