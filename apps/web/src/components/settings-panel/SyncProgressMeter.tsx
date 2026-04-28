import type { AccountOverview } from '../../api/types'
import { cn } from '../../lib/utils'
import { ProgressBar } from '../ui/progress'
import { syncProgressLabel, syncProgressPercent } from './helpers'

export function SyncProgressMeter({
  account,
  compact = false,
}: {
  account: AccountOverview
  compact?: boolean
}) {
  const label = syncProgressLabel(account)
  if (!label) {
    return null
  }

  const percent = syncProgressPercent(account)

  return (
    <ProgressBar
      value={percent}
      label={label}
      compact={compact}
      className={cn(compact ? 'mt-1' : 'rounded-md')}
      indicatorClassName="bg-blue-500"
    />
  )
}
