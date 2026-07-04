/**
 * Compact icon toggle for the per-view "show source mailbox" row chip. Lives
 * beside `ViewModeToggle` in the top chrome; a single pressed/unpressed icon
 * button rather than a segmented control since there are only two states and
 * the icon itself (open vs. closed folder) communicates which one is active.
 *
 * @spec docs/L1-ui#messagelist
 */
import { Folder, FolderOpen } from 'lucide-react'

import { cn } from '@/lib/utils'

import { useShowSourceMailbox } from './useShowSourceMailbox'

export function ShowSourceMailboxToggle({
  viewKey,
  defaultValue,
}: {
  viewKey: string
  defaultValue: boolean
}) {
  const { show, toggleShow } = useShowSourceMailbox(viewKey, defaultValue)
  const Icon = show ? FolderOpen : Folder

  return (
    <button
      type="button"
      title="Show source mailbox"
      aria-label="Show source mailbox"
      aria-pressed={show}
      onClick={toggleShow}
      className={cn(
        'ph-focus-ring inline-flex size-6 shrink-0 items-center justify-center rounded-[6px] border border-border-soft bg-[var(--bg-elev)] transition-colors',
        show
          ? 'text-foreground shadow-sm'
          : 'text-muted-foreground hover:text-foreground',
      )}
    >
      <Icon size={13} strokeWidth={1.7} />
    </button>
  )
}
