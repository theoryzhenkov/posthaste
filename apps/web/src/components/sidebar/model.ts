import type {
  AccountAppearance,
  Mailbox,
  MailboxGroup,
  SmartMailboxSummary,
} from '@/api/types'
import { cn } from '@/lib/utils'

/**
 * Whether a mailbox may be deleted from the sidebar. Protected/role mailboxes
 * (Inbox/Sent/Trash/…) carry a provider-structural `role` and must never be
 * deletable; only a plain user mailbox (`role === null`) is. The delete
 * context-menu item and its confirm dialog are both gated on this.
 *
 * @spec docs/eph/RFC-L2-mailbox-management
 */
export function isMailboxDeletable(mailbox: Mailbox): boolean {
  return mailbox.role == null
}

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

/** One rendered Group within a source: the group and its member mailboxes, in
 *  the source's own mailbox order. */
export interface SourceMailboxGroup {
  group: MailboxGroup
  mailboxes: Mailbox[]
}

/** The partition of a source's mailboxes into client-side Groups + an ungrouped
 *  remainder. Presentation only. */
export interface PartitionedSourceMailboxes {
  /** Mailboxes belonging to no group, in their original order. */
  ungrouped: Mailbox[]
  /** Groups that have at least one member among this source's mailboxes,
   *  ordered by `group.order` (ties broken by name for stability). */
  groups: SourceMailboxGroup[]
}

/**
 * Partition a source's mailboxes into Groups + ungrouped remainder, driven by
 * the synced `mailboxGroups` setting. A Group surfaces under a source only when
 * it holds ≥1 of that source's mailboxes (groups are keyed by member id, not by
 * source). Member order follows the source's own mailbox order — stable and
 * independent of the order ids were assigned. Purely presentational: a mailbox
 * in no group is unaffected.
 *
 * @spec docs/eph/RFC-L2-mailbox-management#a4
 */
export function partitionSourceMailboxes(
  mailboxes: Mailbox[],
  groups: readonly MailboxGroup[],
): PartitionedSourceMailboxes {
  // The group each mailbox id belongs to (one-group-per-mailbox); last write
  // wins if data ever drifts to double-membership.
  const groupByMailboxId = new Map<string, string>()
  for (const group of groups) {
    for (const mailboxId of group.mailboxIds) {
      groupByMailboxId.set(mailboxId, group.id)
    }
  }

  const ungrouped: Mailbox[] = []
  const membersByGroupId = new Map<string, Mailbox[]>()
  for (const mailbox of mailboxes) {
    const groupId = groupByMailboxId.get(mailbox.id)
    if (groupId == null) {
      ungrouped.push(mailbox)
      continue
    }
    const members = membersByGroupId.get(groupId)
    if (members) {
      members.push(mailbox)
    } else {
      membersByGroupId.set(groupId, [mailbox])
    }
  }

  const orderedGroups = [...groups]
    .sort((a, b) => a.order - b.order || a.name.localeCompare(b.name))
    .flatMap((group) => {
      const members = membersByGroupId.get(group.id)
      return members && members.length > 0
        ? [{ group, mailboxes: members }]
        : []
    })

  return { ungrouped, groups: orderedGroups }
}

/**
 * The source's mailboxes in `j`/`k` walk order, honoring Group collapse: every
 * ungrouped mailbox, then each expanded Group's members in order. A collapsed
 * Group contributes nothing — its members are hidden from the walk exactly as a
 * collapsed source hides all of its rows. Shared by the sidebar render and the
 * keyboard walker so they never drift.
 *
 * @spec docs/eph/RFC-L2-mailbox-management#a4
 */
export function visibleSourceMailboxes(
  mailboxes: Mailbox[],
  groups: readonly MailboxGroup[],
  collapsedGroupIds: ReadonlySet<string>,
): Mailbox[] {
  const partition = partitionSourceMailboxes(mailboxes, groups)
  const visible = [...partition.ungrouped]
  for (const entry of partition.groups) {
    if (collapsedGroupIds.has(entry.group.id)) continue
    visible.push(...entry.mailboxes)
  }
  return visible
}

export function itemButtonClass(
  isSelected: boolean,
  depth = 0,
  isPaneActive = false,
): string {
  return cn(
    'mx-1.5 flex h-[var(--density-sidebar-row-height)] w-[calc(100%-0.75rem)] items-center gap-2 rounded-[5px] pr-2 text-left text-[13px] font-medium transition-colors',
    'ph-focus-ring hover:bg-[var(--sidebar-accent)]',
    // The selected mailbox shows the accent only while the sidebar is the
    // focused pane; otherwise it greys out, so accent always means "focused".
    isSelected &&
      isPaneActive &&
      'bg-[var(--list-selection)] text-[var(--list-selection-foreground)]',
    isSelected &&
      !isPaneActive &&
      'bg-[var(--list-selection-muted)] text-[var(--list-selection-muted-foreground)]',
    !isSelected && 'text-sidebar-foreground/92',
    depth > 0 ? 'pl-[22px]' : 'pl-2',
  )
}
