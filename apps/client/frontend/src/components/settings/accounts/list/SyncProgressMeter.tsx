import type { AccountRow } from '@/gen'
import { syncProgressView } from '@/data/models/syncProgress'
import { cn } from '../../../../lib/design/cn'
import { ProgressBar } from '../../../ui/display/progress'

/**
 * Activity bar for an account's running sync.
 *
 * The supervisor publishes live `syncProgress` on the account row for the
 * cycle in flight, so this names the stage — and, on providers that report
 * mailbox position, fills the bar. It falls back to an indeterminate bar
 * labelled "Syncing" when a cycle is running but has not reported yet.
 */
export function SyncProgressMeter({
  account,
  compact = false,
}: {
  account: AccountRow
  compact?: boolean
}) {
  if (account.status !== 'syncing') {
    return null
  }

  const progress = account.syncProgress
  const view = progress ? syncProgressView(progress) : null

  return (
    <ProgressBar
      value={view?.percent ?? null}
      label={view?.label ?? 'Syncing'}
      ariaLabel={view ? `Syncing: ${view.label}` : 'Syncing'}
      compact={compact}
      className={cn(compact ? 'mt-1' : 'rounded-md')}
      indicatorClassName="bg-blue-500"
    />
  )
}
