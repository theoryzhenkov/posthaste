/**
 * Pure mapping between the runtime contract (frames, view state, settlements)
 * and the replica handle's JSON surface. Kept side-effect-free so the host glue
 * is a thin orchestration over tested transforms.
 *
 * @spec docs/replication/L3#5-transport-injection-and-the-contract-replica-mapping
 */
import type {
  RuntimeMailListRowState,
  RuntimeMailListViewState,
  RuntimeMessagePageScope,
  RuntimeMutationSettlementStatus,
} from '../types'
import type { SettlementVerdict } from './handle'

/** One served row mapped to the replica's `{messageId, projection}` shape. */
export interface ReplicaRow {
  messageId: string
  projection: unknown
}

/**
 * The stable message id for a served row. `resourceRef` is `message:{src}:{id}`;
 * the replica keys on the bare `{id}` so it matches the renderer's mutation
 * target. Falls back to the projection's own `id` when no ref is present.
 */
export function messageIdForRow(row: RuntimeMailListRowState): string {
  const ref = row.resourceRef
  if (ref && ref.startsWith('message:')) {
    const parts = ref.split(':')
    const id = parts[parts.length - 1]
    if (id) {
      return id
    }
  }
  return row.projection.id
}

/** Map a served mail-list view state to the replica's base rows. */
export function replicaRowsFromViewState(
  state: RuntimeMailListViewState,
): ReplicaRow[] {
  return state.rows.map((row) => ({
    messageId: messageIdForRow(row),
    projection: row.projection,
  }))
}

/**
 * Rebuild a served view state with the replica's optimistic projections,
 * preserving each surviving row's served metadata (rowKey, resourceRef,
 * orderKey) and overwriting only its presentation projection. Rows the replica
 * dropped (destroyed / archived out) fall away; order follows the projection,
 * which the replica emits in served order.
 */
export function applyOptimisticRows(
  state: RuntimeMailListViewState,
  projections: unknown[],
): RuntimeMailListViewState {
  const byId = new Map(state.rows.map((row) => [row.projection.id, row]))
  const rows = projections.flatMap((projection) => {
    const id = (projection as { id?: string }).id
    const original = id === undefined ? undefined : byId.get(id)
    if (!original) {
      return []
    }
    return [
      {
        ...original,
        projection: projection as RuntimeMailListRowState['projection'],
      },
    ]
  })
  return { ...state, rows }
}

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
