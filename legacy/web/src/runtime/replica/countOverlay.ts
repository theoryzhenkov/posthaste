/**
 * The optimistic count overlay's assertion→adjustment math (D2,
 * RFC-L2-count-unification). Lives in the replica layer because it is shaped
 * by the replica's assertion vocabulary ({@link ReplicaAssertion}); the
 * react-query cache surface it feeds (`adjustMailboxCountsInCache`, the
 * debounced invalidation) lives in `domain-cache/mailboxCounts.ts`.
 */
import type { MailboxCountAdjustment } from '@/domain-cache/mailboxCounts'

import type { ReplicaAssertion } from './handle'

/** The pre-fold message state an adjustment is computed against. */
export interface MessageCountState {
  mailboxIds: readonly string[]
  isRead: boolean
}

const SEEN_KEYWORD = '$seen'

/**
 * Pure: the per-mailbox count adjustments a fold-eligible assertion implies
 * over a message's pre-fold state. Computed uniformly from the before/after
 * membership + read state:
 *
 *   totalDelta  = inAfter - inBefore
 *   unreadDelta = (inAfter && unreadAfter) - (inBefore && unreadBefore)
 *
 * which covers mark read/unread (flip on every holding mailbox), move/archive/
 * trash (both sides), destroy (all holding mailboxes), and applyDiff (undo).
 */
export function mailboxCountAdjustments(
  before: MessageCountState,
  assertion: ReplicaAssertion,
): MailboxCountAdjustment[] {
  const beforeSet = new Set(before.mailboxIds)
  let afterSet: Set<string>
  let isReadAfter = before.isRead
  switch (assertion.kind) {
    case 'setKeywords': {
      afterSet = beforeSet
      if (assertion.remove.includes(SEEN_KEYWORD)) {
        isReadAfter = false
      } else if (assertion.add.includes(SEEN_KEYWORD)) {
        isReadAfter = true
      }
      break
    }
    case 'replaceMailboxes': {
      afterSet = new Set(assertion.mailboxIds)
      break
    }
    case 'destroy': {
      afterSet = new Set()
      break
    }
    case 'applyDiff': {
      afterSet = new Set(beforeSet)
      for (const id of assertion.diff.mailboxes.added) {
        afterSet.add(id)
      }
      for (const id of assertion.diff.mailboxes.removed) {
        afterSet.delete(id)
      }
      if (assertion.diff.keywords.removed.includes(SEEN_KEYWORD)) {
        isReadAfter = false
      } else if (assertion.diff.keywords.added.includes(SEEN_KEYWORD)) {
        isReadAfter = true
      }
      break
    }
  }
  const unreadBefore = before.isRead ? 0 : 1
  const unreadAfter = isReadAfter ? 0 : 1
  const adjustments: MailboxCountAdjustment[] = []
  const affected = new Set([...beforeSet, ...afterSet])
  for (const mailboxId of affected) {
    const inBefore = beforeSet.has(mailboxId) ? 1 : 0
    const inAfter = afterSet.has(mailboxId) ? 1 : 0
    const totalDelta = inAfter - inBefore
    const unreadDelta = inAfter * unreadAfter - inBefore * unreadBefore
    if (totalDelta !== 0 || unreadDelta !== 0) {
      adjustments.push({ mailboxId, unreadDelta, totalDelta })
    }
  }
  return adjustments
}
