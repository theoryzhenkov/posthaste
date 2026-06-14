import type { InfiniteData, QueryClient } from '@tanstack/react-query'

import type {
  ConversationView,
  MessageCommand,
  MessageDetail,
  MessagePage,
  SourceMessageRef,
} from '../api/types'
import { queryKeys } from '../queryKeys'
import { mailKeys } from './keys'
import type { MutableState } from './types'

/** Two string arrays hold the same members regardless of order. */
function sameMembers(a: string[], b: string[]): boolean {
  if (a.length !== b.length) {
    return false
  }
  const set = new Set(a)
  return b.every((value) => set.has(value))
}

/**
 * Read a message's current mutable state from the cache, checking message
 * detail, then any message-list page, then any cached conversation view.
 * Returns null when the message is not present in the cache (the caller then
 * cannot optimistically patch or invert the operation).
 * @spec docs/L1-ui#undo-system
 */
export function captureMutableState(
  queryClient: QueryClient,
  target: SourceMessageRef,
): MutableState | null {
  const detail = queryClient.getQueryData<MessageDetail>(
    mailKeys.message(target.sourceId, target.messageId),
  )
  if (detail) {
    return { keywords: detail.keywords, mailboxIds: detail.mailboxIds }
  }

  for (const [, data] of queryClient.getQueriesData<
    InfiniteData<MessagePage, string | null>
  >({ queryKey: queryKeys.messagesRoot })) {
    const match = data?.pages
      .flatMap((page) => page.items)
      .find(
        (message) =>
          message.sourceId === target.sourceId &&
          message.id === target.messageId,
      )
    if (match) {
      return { keywords: match.keywords, mailboxIds: match.mailboxIds }
    }
  }

  for (const [, conversation] of queryClient.getQueriesData<ConversationView>({
    queryKey: mailKeys.conversationRoot,
  })) {
    const match = conversation?.messages.find(
      (message) =>
        message.sourceId === target.sourceId && message.id === target.messageId,
    )
    if (match) {
      return { keywords: match.keywords, mailboxIds: match.mailboxIds }
    }
  }

  return null
}

/**
 * Produce the minimal set of commands that drives a message from `current` to
 * `target` state. This is the engine behind both forward moves and undo: an
 * inverse is just a diff back to the captured before-image.
 * @spec docs/L1-ui#undo-system
 */
export function diffMutableState(
  current: MutableState,
  target: MutableState,
): MessageCommand[] {
  const commands: MessageCommand[] = []
  if (!sameMembers(current.mailboxIds, target.mailboxIds)) {
    commands.push({ kind: 'replaceMailboxes', mailboxIds: target.mailboxIds })
  }
  const add = target.keywords.filter(
    (keyword) => !current.keywords.includes(keyword),
  )
  const remove = current.keywords.filter(
    (keyword) => !target.keywords.includes(keyword),
  )
  if (add.length > 0 || remove.length > 0) {
    commands.push({ kind: 'setKeywords', add, remove })
  }
  return commands
}
