import type { DomainEvent } from '../api/types'

export function isStringArray(value: unknown): value is string[] {
  return Array.isArray(value) && value.every((item) => typeof item === 'string')
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null
}

/**
 * Structural guard for a wire-shaped {@link DomainEvent} (the envelope the
 * runtime serializes: `crates/posthaste-domain-model/src/model/records.rs`,
 * camelCase serde). Shared by every echo ingress — the link-stream
 * notification frames (`useDaemonEvents`) and the mutation receipt's bundled
 * echo (`entityStoreAdapter.dispatchReceiptEchoEvents`) — so the two paths
 * can never drift on what counts as a dispatchable event.
 */
export function isDomainEventShape(value: unknown): value is DomainEvent {
  if (!isRecord(value)) {
    return false
  }
  return (
    typeof value.seq === 'number' &&
    typeof value.accountId === 'string' &&
    typeof value.topic === 'string' &&
    typeof value.occurredAt === 'string' &&
    isRecord(value.payload)
  )
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
