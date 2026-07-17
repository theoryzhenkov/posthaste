/**
 * Conversation-view data layer: fetches the complete provider thread for
 * every conversation present in the loaded flat list, owns per-conversation
 * collapse state, and builds the flattened tree rows.
 *
 * Threads are fetched complete (including messages outside the current
 * view's filter) so the tree shows the full conversation, matching the
 * conversation-first intent. Fetches are `thread` family queries keyed by
 * the shared family key, so they dedupe with the detail pane.
 *
 * @spec docs/L1-ui#messagelist
 */
import { useCallback, useMemo, useState } from 'react'
import { useQueries } from '@tanstack/react-query'

import type { MessageSummary } from '@/api/types'
import {
  applyAccountNamesToMessages,
  type AccountDirectory,
} from '@/accountDirectory'
import { useMailClient } from '@/data/context'
import { fetchQuery } from '@/data/queries'
import { queryKeys } from '@/data/queryKeys'
import type { ThreadView } from '@/gen'

import {
  buildConversationTree,
  type ConversationTreeRow,
} from './conversationTree'

export function useConversationTree({
  anchors,
  enabled,
  accountDirectory,
}: {
  anchors: MessageSummary[]
  enabled: boolean
  accountDirectory: AccountDirectory
}): {
  rows: ConversationTreeRow[]
  visibleMessages: MessageSummary[]
  /** Toggle the collapse of one node, keyed by its message key. */
  toggleCollapse: (messageKey: string) => void
} {
  const client = useMailClient()
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(
    () => new Set(),
  )

  // One thread fetch per unique provider thread among the anchors; the rows
  // group back into conversations by each returned message's conversationId.
  const threadRefs = useMemo(() => {
    const seen = new Map<string, { accountId: string; threadId: string }>()
    for (const message of anchors) {
      const key = `${message.sourceId}:${message.sourceThreadId}`
      if (!seen.has(key)) {
        seen.set(key, {
          accountId: message.sourceId,
          threadId: message.sourceThreadId,
        })
      }
    }
    return [...seen.values()]
  }, [anchors])

  const threadQueries = useQueries({
    queries: threadRefs.map((ref) => ({
      queryKey: queryKeys.thread(ref),
      queryFn: () => fetchQuery<ThreadView>(client, { thread: ref }),
      enabled,
    })),
  })

  const messagesByConversation = useMemo(() => {
    const map = new Map<string, MessageSummary[]>()
    for (const query of threadQueries) {
      const data = query.data
      if (!data) continue
      for (const message of applyAccountNamesToMessages(
        data.messages,
        accountDirectory,
      )) {
        const bucket = map.get(message.conversationId)
        if (bucket) {
          bucket.push(message)
        } else {
          map.set(message.conversationId, [message])
        }
      }
    }
    return map
    // threadQueries is a fresh array each render; depending on it directly
    // is intentional — the tree rebuild below is cheap relative to the fetches.
  }, [threadQueries, accountDirectory])

  const { rows, visibleMessages } = useMemo(
    () => buildConversationTree({ anchors, messagesByConversation, collapsed }),
    [anchors, messagesByConversation, collapsed],
  )

  const toggleCollapse = useCallback((messageKey: string) => {
    setCollapsed((previous) => {
      const next = new Set(previous)
      if (next.has(messageKey)) {
        next.delete(messageKey)
      } else {
        next.add(messageKey)
      }
      return next
    })
  }, [])

  return { rows, visibleMessages, toggleCollapse }
}
