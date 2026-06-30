/**
 * Conversation-view tree model.
 *
 * Conversation view keeps the same individual message rows as the flat list,
 * but groups them by conversation into a two-level tree: the oldest message of
 * a conversation is the root (depth 0), and every later message is a reply
 * (depth 1) rendered with a left offset. (Message summaries carry no per-message
 * reply parent, so the tree is intentionally two levels, not arbitrary nesting.)
 *
 * Collapsed conversations omit their replies from the flattened rows entirely,
 * so virtualization and keyboard navigation skip them with no special-casing.
 *
 * The builder is pure so it can be unit-tested and so the React layer only owns
 * fetching + collapse state.
 *
 * @spec docs/L1-ui#messagelist
 */
import type { MessageSummary } from '@/api/types'

import { messageKey } from './model'

export interface ConversationTreeRow {
  message: MessageSummary
  /** 0 for the conversation root (oldest message), 1 for replies. */
  depth: number
  conversationId: string
  /** True for the first (root) message of a conversation block. */
  isRoot: boolean
  /** Reply count under the root (depth-1 rows), independent of collapse state. */
  childCount: number
  /** Whether this conversation is collapsed (only meaningful on the root row). */
  collapsed: boolean
}

/** Chronological (oldest-first) ordering, stable on id, matching the detail pane. */
function sortChronologically(messages: MessageSummary[]): MessageSummary[] {
  return [...messages].sort((left, right) => {
    if (left.receivedAt !== right.receivedAt) {
      return left.receivedAt.localeCompare(right.receivedAt)
    }
    return left.id.localeCompare(right.id)
  })
}

function dedupeByKey(messages: MessageSummary[]): MessageSummary[] {
  const byKey = new Map<string, MessageSummary>()
  for (const message of messages) {
    byKey.set(messageKey(message), message)
  }
  return [...byKey.values()]
}

/**
 * Flatten anchor messages into conversation blocks.
 *
 * `anchors` is the view-filtered, sorted flat list; conversation order follows
 * each conversation's first appearance there. Each conversation renders from its
 * complete message set (`messagesByConversation`) when available, falling back
 * to its anchor messages until the full conversation has been fetched.
 */
export function buildConversationTree(input: {
  anchors: MessageSummary[]
  messagesByConversation: Map<string, MessageSummary[]>
  collapsed: ReadonlySet<string>
}): { rows: ConversationTreeRow[]; visibleMessages: MessageSummary[] } {
  const { anchors, messagesByConversation, collapsed } = input

  const anchorsByConversation = new Map<string, MessageSummary[]>()
  for (const anchor of anchors) {
    const bucket = anchorsByConversation.get(anchor.conversationId)
    if (bucket) {
      bucket.push(anchor)
    } else {
      anchorsByConversation.set(anchor.conversationId, [anchor])
    }
  }

  const rows: ConversationTreeRow[] = []
  const seen = new Set<string>()
  for (const anchor of anchors) {
    const conversationId = anchor.conversationId
    if (seen.has(conversationId)) {
      continue
    }
    seen.add(conversationId)

    const complete = messagesByConversation.get(conversationId)
    const source =
      complete && complete.length > 0
        ? complete
        : (anchorsByConversation.get(conversationId) ?? [])
    const ordered = sortChronologically(dedupeByKey(source))
    if (ordered.length === 0) {
      continue
    }

    const isCollapsed = collapsed.has(conversationId)
    const childCount = ordered.length - 1
    rows.push({
      message: ordered[0],
      depth: 0,
      conversationId,
      isRoot: true,
      childCount,
      collapsed: isCollapsed,
    })
    if (!isCollapsed) {
      for (let index = 1; index < ordered.length; index += 1) {
        rows.push({
          message: ordered[index],
          depth: 1,
          conversationId,
          isRoot: false,
          childCount,
          collapsed: false,
        })
      }
    }
  }

  return { rows, visibleMessages: rows.map((row) => row.message) }
}

/** Adapt a flat message list into rows (no tree affordances) for the shared renderer. */
export function flatMessageRows(
  messages: MessageSummary[],
): ConversationTreeRow[] {
  return messages.map((message) => ({
    message,
    depth: 0,
    conversationId: message.conversationId,
    isRoot: false,
    childCount: 0,
    collapsed: false,
  }))
}
