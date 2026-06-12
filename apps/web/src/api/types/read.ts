import type { AccountOverview } from './accounts'
import type { Mailbox, TagSummary } from './mail'
import type { SmartMailboxSummary } from './smartMailboxes'

export type ReadOperation =
  | 'Account/list'
  | 'Mailbox/list'
  | 'SmartMailbox/list'
  | 'Tag/list'

/** @spec docs/L1-api#read-calls */
export type ReadAccountIdSelector = string[] | string

/** @spec docs/L1-api#read-calls */
export interface ReadCall {
  id: string
  op: ReadOperation
  args?: {
    accountIds?: ReadAccountIdSelector
  }
}

/** @spec docs/L1-api#read-calls */
export interface ReadRequest {
  calls: ReadCall[]
}

/** @spec docs/L1-api#read-calls */
export type ReadResult =
  | {
      op: 'Account/list'
      value: { ids: string[]; enabledIds: string[]; items: AccountOverview[] }
    }
  | {
      op: 'Mailbox/list'
      value: { byAccountId: Record<string, Mailbox[]> }
    }
  | { op: 'SmartMailbox/list'; value: { items: SmartMailboxSummary[] } }
  | { op: 'Tag/list'; value: { items: TagSummary[] } }

/** @spec docs/L1-api#read-calls */
export interface ReadResponse {
  results: Record<string, ReadResult>
}
