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

export function smartMailboxPriority(name: string): number {
  const normalized = name.trim().toLowerCase()
  switch (normalized) {
    case 'inbox':
    case 'all inboxes':
      return 0
    case 'flagged':
      return 1
    default:
      return 99
  }
}

export function displaySmartMailboxName(name: string): string {
  return name.trim().toLowerCase() === 'inbox' ? 'All Inboxes' : name
}

export function partitionSmartMailboxes(smartMailboxes: SmartMailboxSummary[]) {
  const quick: SmartMailboxSummary[] = []
  const smart: SmartMailboxSummary[] = []

  for (const mailbox of smartMailboxes) {
    const priority = smartMailboxPriority(mailbox.name)
    if (priority !== 99) {
      quick.push(mailbox)
      continue
    }
    smart.push(mailbox)
  }

  quick.sort(
    (left, right) =>
      smartMailboxPriority(left.name) - smartMailboxPriority(right.name),
  )
  smart.sort((left, right) => left.name.localeCompare(right.name))

  return { quick, smart }
}

export function itemButtonClass(isSelected: boolean, depth = 0): string {
  return cn(
    'mx-1.5 flex h-[28px] w-[calc(100%-0.75rem)] items-center gap-2 rounded-[5px] pr-2 text-left text-[13px] font-medium transition-colors',
    'ph-focus-ring hover:bg-[var(--sidebar-accent)]',
    isSelected &&
      'bg-[var(--list-selection)] text-[var(--list-selection-foreground)]',
    !isSelected && 'text-sidebar-foreground/92',
    depth > 0 ? 'pl-[22px]' : 'pl-2',
  )
}
