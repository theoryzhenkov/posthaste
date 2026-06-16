import {
  fetchConversation,
  fetchMailboxes,
  fetchMessage,
  fetchSearchMessages,
  fetchSmartMailboxMessages,
  fetchSmartMailboxes,
  fetchSourceMessages,
  performMessageCommand,
  read,
} from '../api/client'

import type { RuntimeAdapter, RuntimeMessagePageRequest } from './types'

/**
 * Default runtime adapter during migration.
 *
 * It preserves production behavior by delegating to the existing typed HTTP
 * client while renderer code moves behind the runtime facade.
 */
function currentBackendSort(sort: RuntimeMessagePageRequest['sort']) {
  return sort === 'relevance' ? undefined : sort
}

export const httpRuntimeAdapter: RuntimeAdapter = {
  fetchConversation(conversationId) {
    return fetchConversation(conversationId)
  },
  fetchMailboxes(accountId) {
    return fetchMailboxes(accountId)
  },
  fetchMessage(messageId, sourceId) {
    return fetchMessage(messageId, sourceId)
  },
  fetchMessagePage(req) {
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
  fetchSmartMailboxes() {
    return fetchSmartMailboxes()
  },
  read(request) {
    return read(request)
  },
  runMessageCommand({ command, messageId, sourceId }) {
    return performMessageCommand(messageId, command, sourceId)
  },
}
