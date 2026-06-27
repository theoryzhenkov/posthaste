/**
 * Pure mapping between the runtime contract (frames, view state, settlements)
 * and the entity-store handle's JSON surface. Kept side-effect-free so the host
 * glue is a thin orchestration over tested transforms.
 *
 * @spec docs/eph/DESIGN-L2-client-link-reactive-store (2e)
 */
import type { RuntimeMutationSettlementStatus } from '../types'
import type { SettlementVerdict } from './handle'

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
