import type { MailQueryRule } from './mailQuery'

export type SmartMailboxKind = 'default' | 'user'

/** @spec docs/L1-api#smart-mailbox-crud */
export interface SmartMailbox {
  id: string
  name: string
  kind: SmartMailboxKind
  defaultKey: string | null
  /** The mailbox role whose semantics apply to this view (e.g. 'trash'),
   *  driving contextual actions like Delete Permanently. `null` for All Mail
   *  and unassigned user smart mailboxes. */
  role: string | null
  parentId: string | null
  rule: MailQueryRule
  createdAt: string
  updatedAt: string
}

/** @spec docs/L1-api#smart-mailbox-crud */
export interface SmartMailboxSummary {
  id: string
  name: string
  kind: SmartMailboxKind
  defaultKey: string | null
  role: string | null
  parentId: string | null
  unreadMessages: number
  totalMessages: number
  createdAt: string
  updatedAt: string
}

export interface CreateSmartMailboxInput {
  name: string
  /** Optional view role (e.g. 'archive') giving the smart mailbox a built-in
   *  role's icon/accent and contextual actions. */
  role?: string | null
  rule: MailQueryRule
}

/** @spec docs/L1-api#smart-mailbox-crud */
export interface UpdateSmartMailboxInput {
  name?: string
  /** Set a role, or pass an empty string to clear it. Omit to leave unchanged. */
  role?: string | null
  rule?: MailQueryRule
}
