/**
 * Keep `queryKeys.accounts` fed from the runtime `accountStatus` view (the
 * folded config + runtime account list), so account status reaches the renderer
 * as a re-served snapshot rather than patched `account.status_changed` deltas.
 *
 * A missed delta can strand an account in a stale state; a served snapshot
 * cannot, because every account change re-serves the full current list and
 * open/reconnect yields the current snapshot.
 *
 * @spec docs/runtime/L2#account-status-views
 */
import type { AccountOverview } from '@/api/types'
import { queryKeys } from '@/queryKeys'
import { useRuntimeObjectView } from '@/runtime/useRuntimeObjectView'

export function useAccountsView() {
  useRuntimeObjectView<AccountOverview[]>({
    enabled: true,
    family: 'accountStatus',
    payload: {},
    queryKey: queryKeys.accounts,
    sourceId: null,
  })
}
