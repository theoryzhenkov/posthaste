/**
 * Pure mapping between the runtime contract (frames, view state, settlements)
 * and the entity-store handle's JSON surface. Kept side-effect-free so the host
 * glue is a thin orchestration over tested transforms.
 *
 * @spec docs/eph/DESIGN-L2-client-link-reactive-store (2e)
 */
import type { RuntimeMutationNotification } from '../types'
import type { SettlementVerdict } from './handle'

/**
 * The store's settle verdict for a `mutationNotification` body: `confirmed`
 * retires the op by absorption (no revert); `rejected` reverts it. Maps onto the
 * WASM boundary's `SettlementVerdict` (`'failed'` is its term for a rejection).
 */
export function settlementVerdict(
  notification: RuntimeMutationNotification,
): SettlementVerdict {
  switch (notification.type) {
    case 'confirmed':
      return 'confirmed'
    case 'rejected':
      return 'failed'
  }
}
