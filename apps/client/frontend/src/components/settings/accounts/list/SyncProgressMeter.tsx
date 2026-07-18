import type { AccountRow } from '@/gen'
import { cn } from '../../../../lib/design/cn'
import { ProgressBar } from '../../../ui/display/progress'

/**
 * Indeterminate activity bar shown while an account is syncing. The accounts
 * family reports the syncing state only — per-mailbox progress detail is not
 * part of the API surface.
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

  return (
    <ProgressBar
      value={null}
      label="Syncing"
      compact={compact}
      className={cn(compact ? 'mt-1' : 'rounded-md')}
      indicatorClassName="bg-blue-500"
    />
  )
}
