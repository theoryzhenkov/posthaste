import {
  fetchSearchMessages,
  fetchSmartMailboxMessages,
  fetchSourceMessages,
} from './api/client'
import type { MessagePage, MessageSortField } from './api/types'
import type { OperationContext } from './observability'

export type MessagePageScope =
  | { kind: 'source-mailbox'; sourceId: string; mailboxId: string | null }
  | { kind: 'smart-mailbox'; smartMailboxId: string }
  | { kind: 'global' }

export interface MessagePageRequest {
  scope: MessagePageScope
  query?: string
  cursor?: string | null
  limit: number
  sort?: MessageSortField | 'relevance'
  sortDir?: 'asc' | 'desc'
  signal?: AbortSignal
  operation: OperationContext
}

export interface MessagePageClient {
  fetchPage(req: MessagePageRequest): Promise<MessagePage>
}

function currentBackendSort(sort: MessagePageRequest['sort']) {
  return sort === 'relevance' ? undefined : sort
}

export const messagePageClient: MessagePageClient = {
  fetchPage(req) {
    const input = {
      q: req.query,
      cursor: req.cursor,
      limit: req.limit,
      sort: currentBackendSort(req.sort),
      sortDir: req.sortDir,
      signal: req.signal,
      operation: req.operation,
    }

    switch (req.scope.kind) {
      case 'source-mailbox':
        return fetchSourceMessages(
          req.scope.sourceId,
          req.scope.mailboxId,
          input,
        )
      case 'smart-mailbox':
        return fetchSmartMailboxMessages(req.scope.smartMailboxId, input)
      case 'global':
        if (!req.query?.trim()) {
          return Promise.resolve({ items: [], nextCursor: null })
        }
        return fetchSearchMessages(req.query, input)
    }
  },
}
