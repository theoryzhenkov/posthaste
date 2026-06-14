import type { DomainEvent } from '../api/types'

export function isStringArray(value: unknown): value is string[] {
  return Array.isArray(value) && value.every((item) => typeof item === 'string')
}

export function payloadString(
  payload: DomainEvent['payload'],
  key: string,
): string | undefined {
  const value = payload[key]
  return typeof value === 'string' ? value : undefined
}

export function payloadConversationId(
  payload: DomainEvent['payload'],
): string | null {
  return typeof payload.conversationId === 'string'
    ? payload.conversationId
    : null
}

export function eventTarget(event: DomainEvent) {
  return event.messageId && event.accountId
    ? { messageId: event.messageId, sourceId: event.accountId }
    : null
}
