import type { InfiniteData, QueryClient } from '@tanstack/react-query'

import type { MailListResult, MessageSummary } from '@/gen'

function isMailListResult(value: unknown): value is MailListResult {
  return (
    typeof value === 'object' &&
    value !== null &&
    Array.isArray((value as { rows?: unknown }).rows)
  )
}

function isInfiniteMailList(
  value: unknown,
): value is InfiniteData<MailListResult> {
  return (
    typeof value === 'object' &&
    value !== null &&
    Array.isArray((value as { pages?: unknown }).pages)
  )
}

/**
 * The palette's zero-query seed: rows from whatever `mailList` answers the
 * mirror currently holds. A read of cached query answers only — no fetch, no
 * ordering guarantees beyond each answer's own.
 */
export function recentCachedMessages(
  queryClient: QueryClient,
  limit = 12,
): MessageSummary[] {
  const seen = new Set<string>()
  const messages: MessageSummary[] = []
  // Family-prefix read over the flat [family, canonicalArgs] key scheme.
  const cached = queryClient.getQueriesData<unknown>({
    queryKey: ['mailList'],
  })

  for (const [, value] of cached) {
    const pages = isInfiniteMailList(value)
      ? value.pages
      : isMailListResult(value)
        ? [value]
        : []
    for (const page of pages) {
      for (const message of page.rows) {
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
