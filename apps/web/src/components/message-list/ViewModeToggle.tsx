/**
 * Segmented control to switch a view between the flat message list and the
 * conversation (nested-tree) view. Mode is persisted per view.
 *
 * @spec docs/L1-ui#messagelist
 */
import { List, ListTree } from 'lucide-react'

import { cn } from '@/lib/utils'

import type { MessageListViewMode } from './useViewMode'

const OPTIONS: {
  mode: MessageListViewMode
  label: string
  icon: typeof List
}[] = [
  { mode: 'messages', label: 'Messages', icon: List },
  { mode: 'conversations', label: 'Conversations', icon: ListTree },
]

export function ViewModeToggle({
  mode,
  onChange,
}: {
  mode: MessageListViewMode
  onChange: (mode: MessageListViewMode) => void
}) {
  return (
    <div className="inline-flex items-center gap-0.5 rounded-[6px] border border-border-soft bg-[var(--bg-elev)] p-0.5">
      {OPTIONS.map((option) => {
        const Icon = option.icon
        const active = option.mode === mode
        return (
          <button
            key={option.mode}
            type="button"
            title={`${option.label} view`}
            aria-pressed={active}
            onClick={() => onChange(option.mode)}
            className={cn(
              'ph-focus-ring inline-flex h-6 items-center gap-1.5 rounded-[4px] px-2 text-[11px] font-medium transition-colors',
              active
                ? 'bg-panel text-foreground shadow-sm'
                : 'text-muted-foreground hover:text-foreground',
            )}
          >
            <Icon size={13} strokeWidth={1.7} />
            {option.label}
          </button>
        )
      })}
    </div>
  )
}
