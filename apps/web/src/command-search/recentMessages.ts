import type { InfiniteData, QueryClient } from '@tanstack/react-query'

import type { MessagePage, MessageSummary } from '@/api/types'
import { queryKeys } from '@/queryKeys'

function isMessagePage(value: unknown): value is MessagePage {
  return (
    typeof value === 'object' &&
    value !== null &&
    Array.isArray((value as { items?: unknown }).items)
  )
}

function isInfiniteMessagePage(
  value: unknown,
): value is InfiniteData<MessagePage> {
  return (
    typeof value === 'object' &&
    value !== null &&
    Array.isArray((value as { pages?: unknown }).pages)
  )
}

export function recentCachedMessages(
  queryClient: QueryClient,
  limit = 12,
): MessageSummary[] {
  const seen = new Set<string>()
  const messages: MessageSummary[] = []
  const cached = queryClient.getQueriesData<unknown>({
    queryKey: queryKeys.messagesRoot,
  })

  for (const [, value] of cached) {
    const pages = isInfiniteMessagePage(value)
      ? value.pages
      : isMessagePage(value)
        ? [value]
        : []
    for (const page of pages) {
      for (const message of page.items) {
        const key = `${message.sourceId}:${message.id}`
        if (seen.has(key)) continue
        seen.add(key)
        messages.push(message)
        if (messages.length >= limit) return messages
      }
    }
  }

  return messages
}
