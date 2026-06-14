import type { DomainEvent } from '../api/types'

const LOCAL_MUTATION_TTL_MS = 5_000
const localMutationEvents = new Map<string, number>()

function buildLocalMutationKey(
  event: Pick<DomainEvent, 'accountId' | 'messageId' | 'topic'>,
) {
  return `${event.accountId}:${event.messageId ?? 'none'}:${event.topic}`
}

function cleanupLocalMutationEvents(now: number) {
  for (const [key, expiresAt] of localMutationEvents) {
    if (expiresAt <= now) {
      localMutationEvents.delete(key)
    }
  }
}

/**
 * Record events from a locally initiated mutation so they can be
 * suppressed when echoed back via SSE.
 * @spec docs/L1-ui#live-prepend-behavior
 */
export function recordLocalMutationEvents(events: DomainEvent[]) {
  const now = Date.now()
  cleanupLocalMutationEvents(now)
  for (const event of events) {
    if (!event.messageId) {
      continue
    }
    localMutationEvents.set(
      buildLocalMutationKey(event),
      now + LOCAL_MUTATION_TTL_MS,
    )
  }
}

/**
 * Returns true if this SSE event was caused by a recent local mutation
 * and should be ignored to prevent double-application.
 * @spec docs/L1-ui#live-prepend-behavior
 */
export function shouldSuppressLocalEcho(event: DomainEvent): boolean {
  if (!event.messageId) {
    return false
  }

  const now = Date.now()
  cleanupLocalMutationEvents(now)
  const key = buildLocalMutationKey(event)
  const expiresAt = localMutationEvents.get(key)
  if (!expiresAt || expiresAt <= now) {
    localMutationEvents.delete(key)
    return false
  }
  localMutationEvents.delete(key)
  return true
}
