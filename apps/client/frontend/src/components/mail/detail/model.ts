import type { MessageSummary, Recipient } from '@/data/transport/api'

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

export function formatRecipientEmailList(recipients: Recipient[]): string {
  if (recipients.length === 0) {
    return 'recipients unavailable'
  }
  return recipients.map((recipient) => recipient.email).join(', ')
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
