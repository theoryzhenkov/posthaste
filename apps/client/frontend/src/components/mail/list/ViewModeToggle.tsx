/**
 * Segmented control to switch the active view between the flat message list and
 * the conversation (nested-tree) view. Lives in the app's top chrome next to
 * command search; mode is per-view and persisted via the shared `useViewMode`
 * store, so it stays in sync with the list it controls.
 *
 */
import { List, ListTree } from 'lucide-react'

import { cn } from '@/lib/cn'

import { useViewMode, type MessageListViewMode } from './model/useViewMode'

const OPTIONS: {
  mode: MessageListViewMode
  label: string
  icon: typeof List
}[] = [
  { mode: 'messages', label: 'Messages', icon: List },
  { mode: 'conversations', label: 'Conversations', icon: ListTree },
]

export function ViewModeToggle({ viewModeKey }: { viewModeKey: string }) {
  const { mode, setMode } = useViewMode(viewModeKey)

  return (
    <div
      data-tour-anchor="conversation-view"
      className="inline-flex items-center gap-0.5 rounded-[6px] border border-border-soft bg-[var(--bg-elev)] p-0.5"
    >
      {OPTIONS.map((option) => {
        const Icon = option.icon
        const active = option.mode === mode
        return (
          <button
            key={option.mode}
            type="button"
            title={`${option.label} view`}
            aria-label={`${option.label} view`}
            aria-pressed={active}
            onClick={() => setMode(option.mode)}
            className={cn(
              'ph-focus-ring inline-flex size-6 items-center justify-center rounded-[4px] transition-colors',
              active
                ? 'bg-panel text-foreground shadow-sm'
                : 'text-muted-foreground hover:text-foreground',
            )}
          >
            <Icon size={13} strokeWidth={1.7} />
          </button>
        )
      })}
    </div>
  )
}
