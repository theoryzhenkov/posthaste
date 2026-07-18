import { cn } from '@/lib/cn'
import { normalizeProgressValue } from '@/components/ui/display/progressValue'

interface ProgressBarProps {
  value?: number | null
  label?: string
  ariaLabel?: string
  compact?: boolean
  className?: string
  labelClassName?: string
  trackClassName?: string
  indicatorClassName?: string
}

export function ProgressBar({
  value,
  label,
  ariaLabel,
  compact = false,
  className,
  labelClassName,
  trackClassName,
  indicatorClassName,
}: ProgressBarProps) {
  const normalizedValue = normalizeProgressValue(value)
  const isIndeterminate = normalizedValue === null

  return (
    <div className={cn('min-w-0', className)}>
      {label && (
        <div
          className={cn(
            'truncate text-muted-foreground',
            compact ? 'text-[11px]' : 'text-[12px]',
            labelClassName,
          )}
        >
          {label}
        </div>
      )}
      <div
        aria-label={ariaLabel ?? label ?? 'Progress'}
        aria-valuemax={100}
        aria-valuemin={0}
        aria-valuenow={isIndeterminate ? undefined : normalizedValue}
        aria-valuetext={isIndeterminate ? 'In progress' : undefined}
        className={cn(
          'overflow-hidden rounded-full bg-border-soft',
          compact ? 'mt-1 h-1' : 'mt-2 h-1.5',
          trackClassName,
        )}
        role="progressbar"
      >
        <div
          className={cn(
            'h-full rounded-full bg-brand-coral transition-[width] duration-300',
            isIndeterminate && 'ph-progress-indeterminate',
            indicatorClassName,
          )}
          style={isIndeterminate ? undefined : { width: `${normalizedValue}%` }}
        />
      </div>
    </div>
  )
}
