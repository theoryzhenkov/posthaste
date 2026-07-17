/**
 * Conversation-view tree model.
 *
 * Conversation view keeps the same individual message rows as the flat list,
 * but groups them by conversation into a real reply tree: the oldest message is
 * the root (depth 0, no offset), and every later message hangs under the parent
 * it replied to — resolved by matching its `inReplyTo` to another message's
 * `rfcMessageId` within the thread. Depth is the length of that parent chain, so
 * rows are offset by how deep the reply sits. Messages whose parent isn't in the
 * thread (missing/broken headers) fall back to the conversation root.
 *
 * Collapse is per-message: collapsing any node with children omits its whole
 * subtree from the flattened rows, so virtualization and keyboard navigation
 * skip them with no special-casing.
 *
 * The builder is pure so it can be unit-tested and so the React layer only owns
 * fetching + collapse state.
 *
 */
import type { MessageSummary } from '@/api/types'

import { messageKey } from './model'

export interface ConversationTreeRow {
  message: MessageSummary
  /** Depth in the reply tree: 0 for the conversation root (no offset), +1 per
   *  reply level. */
  depth: number
  conversationId: string
  /** Whether this message has replies under it — i.e. shows a collapse chevron. */
  hasChildren: boolean
  /** Whether this node is collapsed (its subtree is hidden). */
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
 * Build one conversation's reply tree into `out` (preorder: parent before its
 * children, siblings chronological). The oldest message is the root; each later
 * message's parent is the in-thread message it replied to (`inReplyTo` →
 * `rfcMessageId`), or the root when that parent isn't present. Requiring the
 * parent to be strictly older keeps the parent map a tree (no cycles).
 */
function buildConversationBlock(
  messages: MessageSummary[],
  collapsed: ReadonlySet<string>,
  out: ConversationTreeRow[],
): void {
  const ordered = sortChronologically(dedupeByKey(messages))
  if (ordered.length === 0) {
    return
  }
  const conversationId = ordered[0].conversationId

  const indexByKey = new Map<string, number>()
  ordered.forEach((message, index) =>
    indexByKey.set(messageKey(message), index),
  )

  const messageByRfcId = new Map<string, MessageSummary>()
  for (const message of ordered) {
    if (message.rfcMessageId) {
      messageByRfcId.set(message.rfcMessageId, message)
    }
  }

  const root = ordered[0]
  const rootKey = messageKey(root)

  // Children of each node, in the chronological order we discover them.
  const children = new Map<string, MessageSummary[]>()
  for (let index = 1; index < ordered.length; index += 1) {
    const message = ordered[index]
    const key = messageKey(message)
    const candidate = message.inReplyTo
      ? messageByRfcId.get(message.inReplyTo)
      : undefined
    const candidateKey = candidate ? messageKey(candidate) : null
    const parentKey =
      candidateKey !== null &&
      candidateKey !== key &&
      (indexByKey.get(candidateKey) ?? Infinity) < index
        ? candidateKey
        : rootKey
    const bucket = children.get(parentKey)
    if (bucket) {
      bucket.push(message)
    } else {
      children.set(parentKey, [message])
    }
  }

  const visit = (message: MessageSummary, depth: number): void => {
    const key = messageKey(message)
    const kids = children.get(key) ?? []
    const isCollapsed = collapsed.has(key)
    out.push({
      message,
      depth,
      conversationId,
      hasChildren: kids.length > 0,
      collapsed: isCollapsed,
    })
    if (!isCollapsed) {
      for (const child of kids) {
        visit(child, depth + 1)
      }
    }
  }
  visit(root, 0)
}

/**
 * Flatten anchor messages into conversation reply trees.
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
    buildConversationBlock(source, collapsed, rows)
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
    hasChildren: false,
    collapsed: false,
  }))
}
