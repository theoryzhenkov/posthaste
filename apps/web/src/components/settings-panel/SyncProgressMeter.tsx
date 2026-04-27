import type { AccountOverview } from '../../api/types'
import { cn } from '../../lib/utils'
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
  const widthStyle = percent === null ? undefined : { width: `${percent}%` }

  return (
    <div className={cn('min-w-0', compact ? 'mt-1' : 'rounded-md')}>
      <div
        className={cn(
          'truncate text-muted-foreground',
          compact ? 'text-[11px]' : 'text-[12px]',
        )}
      >
        {label}
      </div>
      <div
        className={cn(
          'overflow-hidden rounded-full bg-border-soft',
          compact ? 'mt-1 h-1' : 'mt-2 h-1.5',
        )}
      >
        <div
          className={cn(
            'h-full rounded-full bg-blue-500 transition-[width] duration-300',
            percent === null && 'w-1/3 animate-pulse',
          )}
          style={widthStyle}
        />
      </div>
    </div>
  )
}
