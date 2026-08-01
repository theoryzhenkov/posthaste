import type { MessageSummary } from '@/data/transport/api'

/**
 * Marks the reading pane's body scroll container. The pane is display-only
 * (never a keyboard focus region), so the keyboard controller pages the
 * element carrying this attribute on Space / Shift+Space.
 */
export const MESSAGE_SCROLL_ATTRIBUTE = 'data-message-scroll'

export function userTags(keywords: string[]): string[] {
  return keywords.filter((kw) => !kw.startsWith('$'))
}

export function dedupeConversationMessages(
  messages: MessageSummary[],
): MessageSummary[] {
  const uniqueMessages = new Map<string, MessageSummary>()
  for (const message of messages) {
    uniqueMessages.set(`${message.sourceId}:${message.id}`, message)
  }
  return [...uniqueMessages.values()].sort((left, right) => {
    if (left.receivedAt !== right.receivedAt) {
      return left.receivedAt.localeCompare(right.receivedAt)
    }
    return left.id.localeCompare(right.id)
  })
}

export function formatAbsoluteDate(value: string): string {
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: 'medium',
    timeStyle: 'short',
  }).format(new Date(value))
}

export function initialsForSender(senderName: string): string {
  return (
    senderName
      .split(/\s+/)
      .filter(Boolean)
      .slice(0, 2)
      .map((part) => part[0]?.toUpperCase())
      .join('') || '?'
  )
}
