import type { Mailbox, MessageSummary, TagSummary } from '../api/types'

export interface QueryCompletion {
  id: string
  label: string
  replacement: string
  detail: string
  kind: 'prefix' | 'value'
}

export type QueryValidation =
  | { state: 'valid' }
  | { state: 'incomplete'; message: string }
  | { state: 'invalid'; message: string }

export interface QueryCompletionSource {
  id: string
  name: string
  mailboxes: Mailbox[]
}

export interface QueryCompletionContext {
  messages: MessageSummary[]
  now?: Date
  sources: QueryCompletionSource[]
  tags: TagSummary[]
}

export interface ValueCandidate {
  value: string
  label: string
  detail: string
  keywords?: string
}
