import type { AccountAppearance, SmartMailboxSummary } from '@/api/types'
import { cn } from '@/lib/utils'

export function fallbackAccountAppearance(
  sourceId: string,
  sourceName: string,
): AccountAppearance {
  const seed = `${sourceId}:${sourceName}`
  let hash = 0
  for (let index = 0; index < seed.length; index += 1) {
    hash = (hash * 31 + seed.charCodeAt(index)) >>> 0
  }
  return {
    kind: 'initials',
    initials: sourceName.trim().charAt(0).toUpperCase() || '?',
    colorHue: hash % 361,
  }
}

/** Smart mailboxes as the backend resolved them: the user's explicit
 * `smartMailboxOrder` first, then the canonical/default fallback. The renderer
 * preserves this order verbatim. */
export function sortSmartMailboxes(
  smartMailboxes: SmartMailboxSummary[],
): SmartMailboxSummary[] {
  return smartMailboxes
}

export function itemButtonClass(isSelected: boolean, depth = 0): string {
  return cn(
    'mx-1.5 flex h-[var(--density-sidebar-row-height)] w-[calc(100%-0.75rem)] items-center gap-2 rounded-[5px] pr-2 text-left text-[13px] font-medium transition-colors',
    'ph-focus-ring hover:bg-[var(--sidebar-accent)]',
    isSelected &&
      'bg-[var(--list-selection)] text-[var(--list-selection-foreground)]',
    !isSelected && 'text-sidebar-foreground/92',
    depth > 0 ? 'pl-[22px]' : 'pl-2',
  )
}
