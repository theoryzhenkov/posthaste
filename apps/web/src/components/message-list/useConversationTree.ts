/**
 * Conversation-view data layer: fetches the complete thread for every
 * conversation present in the loaded flat list, owns per-conversation collapse
 * state, and builds the flattened tree rows.
 *
 * Conversations are fetched complete (the whole thread, including messages
 * outside the current view's filter) so the tree shows the full conversation,
 * matching the conversation-first intent. Fetches are keyed by the shared
 * conversation cache key, so they dedupe with the detail pane.
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
import { mailKeys } from '@/mailState'
import { runtimeViews } from '@/runtime/views'

import {
  buildConversationTree,
  type ConversationTreeRow,
} from './conversationTree'

const CONVERSATION_STALE_MS = 30_000

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
  toggleCollapse: (conversationId: string) => void
} {
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(
    () => new Set(),
  )

  const conversationIds = useMemo(() => {
    const seen = new Set<string>()
    for (const message of anchors) {
      seen.add(message.conversationId)
    }
    return [...seen]
  }, [anchors])

  const conversationQueries = useQueries({
    queries: conversationIds.map((conversationId) => ({
      queryKey: mailKeys.conversation(conversationId),
      queryFn: () => runtimeViews.mail.conversation(conversationId),
      enabled,
      staleTime: CONVERSATION_STALE_MS,
    })),
  })

  const messagesByConversation = useMemo(() => {
    const map = new Map<string, MessageSummary[]>()
    conversationQueries.forEach((query, index) => {
      const data = query.data
      if (data) {
        map.set(
          conversationIds[index],
          applyAccountNamesToMessages(data.messages, accountDirectory),
        )
      }
    })
    return map
    // conversationQueries is a fresh array each render; depending on it directly
    // is intentional — the tree rebuild below is cheap relative to the fetches.
  }, [conversationQueries, conversationIds, accountDirectory])

  const { rows, visibleMessages } = useMemo(
    () => buildConversationTree({ anchors, messagesByConversation, collapsed }),
    [anchors, messagesByConversation, collapsed],
  )

  const toggleCollapse = useCallback((conversationId: string) => {
    setCollapsed((previous) => {
      const next = new Set(previous)
      if (next.has(conversationId)) {
        next.delete(conversationId)
      } else {
        next.add(conversationId)
      }
      return next
    })
  }, [])

  return { rows, visibleMessages, toggleCollapse }
}
