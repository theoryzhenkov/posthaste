import type { MailQueryRule } from './mailQuery'

// The smart-mailbox projection the backend answers with is the generated
// `SmartMailboxRow` twin (configuration incl. rule + live counts),
// re-exported under both historical names so the whole tree shares one type
// identity.
export type { SmartMailboxKind } from '@/gen'
export type { SmartMailboxRow as SmartMailbox } from '@/gen'
export type { SmartMailboxRow as SmartMailboxSummary } from '@/gen'

export interface CreateSmartMailboxInput {
  name: string
  /** Optional view role (e.g. 'archive') giving the smart mailbox a built-in
   *  role's icon/accent and contextual actions. */
  role?: string | null
  rule: MailQueryRule
}

export interface UpdateSmartMailboxInput {
  name?: string
  /** Set a role, or pass an empty string to clear it. Omit to leave unchanged. */
  role?: string | null
  rule?: MailQueryRule
}
