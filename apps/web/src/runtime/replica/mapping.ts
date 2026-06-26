/**
 * Pure mapping between the runtime contract (frames, view state, settlements)
 * and the replica handle's JSON surface. Kept side-effect-free so the host glue
 * is a thin orchestration over tested transforms.
 *
 * @spec docs/replication/client-link/L3#4-transport-injection-and-the-contract-replica-mapping
 */
import type {
  RuntimeMessagePageScope,
  RuntimeMutationSettlementStatus,
} from '../types'
import type { SettlementVerdict } from './handle'

/**
 * The concrete mailbox a `projectJson` call should filter membership against,
 * or `null` to defer membership to the runtime's next served base. Only a
 * single concrete source mailbox yields instant archive-out; smart/global
 * scopes need full local query evaluation (the deferred coverage layer).
 */
export function membershipMailbox(
  scope: RuntimeMessagePageScope,
): string | null {
  return scope.kind === 'source-mailbox' ? scope.mailboxId : null
}

/**
 * The replica verdict for a settlement status, or `null` when the status is
 * non-terminal (`accepted` / `localApplied` / `queued`) and must be ignored.
 */
export function settlementVerdict(
  status: RuntimeMutationSettlementStatus,
): SettlementVerdict | null {
  switch (status) {
    case 'confirmed':
      return 'confirmed'
    case 'failed':
    case 'conflict':
      return 'failed'
    case 'accepted':
    case 'localApplied':
    case 'queued':
      return null
  }
}
