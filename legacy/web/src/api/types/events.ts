import type { MessageDetail } from './mail'

export interface DomainEvent {
  seq: number
  accountId: string
  topic: string
  occurredAt: string
  mailboxId: string | null
  messageId: string | null
  payload: Record<string, unknown>
}

/** @spec docs/L1-api#endpoint-table */
export interface MessageCommandResult {
  detail: MessageDetail | null
  events: DomainEvent[]
}

/**
 * Discriminated union of all message commands the API accepts.
 * @spec docs/L1-api#endpoint-table
 */
export type MessageCommand =
  | { kind: 'setKeywords'; add: string[]; remove: string[] }
  | { kind: 'addToMailbox'; mailboxId: string }
  | { kind: 'removeFromMailbox'; mailboxId: string }
  | { kind: 'replaceMailboxes'; mailboxIds: string[] }
  | { kind: 'destroy' }
