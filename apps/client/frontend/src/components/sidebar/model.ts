import type {
  AccountAppearance,
  Mailbox,
  MailboxGroup,
  SmartMailboxSummary,
} from '@/data/transport/api'
import { cn } from '@/lib/design/cn'

/**
 * Whether a mailbox may be deleted from the sidebar. Protected/role mailboxes
 * (Inbox/Sent/Trash/…) carry a provider-structural `role` and must never be
 * deletable; only a plain user mailbox (`role === null`) is. The delete
 * context-menu item and its confirm dialog are both gated on this.
 */
export function isMailboxDeletable(mailbox: Mailbox): boolean {
  return mailbox.role == null
}

/**
 * Whether a mailbox may be renamed from the sidebar. Same protection rule as
 * delete: role mailboxes are provider-structural and keep their names; only a
 * plain user mailbox (`role === null`) offers the rename affordance.
 */
export function isMailboxRenamable(mailbox: Mailbox): boolean {
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

/** One rendered Group within a section: the group and its member items, in
 *  the section's own item order. */
interface MailboxGroupEntry<T> {
  group: MailboxGroup
  mailboxes: T[]
}

/** The partition of a section's items into client-side Groups + an ungrouped
 *  remainder. Presentation only. */
interface PartitionedMailboxes<T> {
  /** Items belonging to no group, in their original order. */
  ungrouped: T[]
  /** Groups that have at least one member in this section, ordered by
   *  `group.order` (ties broken by name for stability). */
  groups: MailboxGroupEntry<T>[]
}

export type PartitionedSourceMailboxes = PartitionedMailboxes<Mailbox>
export type PartitionedSmartMailboxes = PartitionedMailboxes<SmartMailboxSummary>

/**
 * Partition a section's items into Groups + ungrouped remainder, driven by the
 * synced `mailboxGroups` setting. A Group surfaces in a section only when it
 * holds >=1 of that section's items (groups are keyed by member id, not by
 * section) — iterating over the section's own items IS the membership filter,
 * so a mixed or foreign group shows only (or none of) its members here. Member
 * order follows the section's own item order — stable and independent of the
 * order ids were assigned. Purely presentational: an item in no group is
 * unaffected. Group ids share one namespace across the Smart and source
 * sections; membership is homogeneous per the UI's enforcement.
 */
function partitionByGroup<T extends { id: string }>(
  items: T[],
  groups: readonly MailboxGroup[],
): PartitionedMailboxes<T> {
  // The group each item id belongs to (one-group-per-item); last write wins
  // if data ever drifts to double-membership.
  const groupByMailboxId = new Map<string, string>()
  for (const group of groups) {
    for (const mailboxId of group.mailboxIds) {
      groupByMailboxId.set(mailboxId, group.id)
    }
  }

  const ungrouped: T[] = []
  const membersByGroupId = new Map<string, T[]>()
  for (const item of items) {
    const groupId = groupByMailboxId.get(item.id)
    if (groupId == null) {
      ungrouped.push(item)
      continue
    }
    const members = membersByGroupId.get(groupId)
    if (members) {
      members.push(item)
    } else {
      membersByGroupId.set(groupId, [item])
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

export function partitionSourceMailboxes(
  mailboxes: Mailbox[],
  groups: readonly MailboxGroup[],
): PartitionedSourceMailboxes {
  return partitionByGroup(mailboxes, groups)
}

export function partitionSmartMailboxes(
  smartMailboxes: SmartMailboxSummary[],
  groups: readonly MailboxGroup[],
): PartitionedSmartMailboxes {
  return partitionByGroup(smartMailboxes, groups)
}

/**
 * A section's items in `j`/`k` walk order, honoring Group collapse: every
 * ungrouped item, then each expanded Group's members in order. A collapsed
 * Group contributes nothing — its members are hidden from the walk exactly as
 * a collapsed source hides all of its rows. Shared by the sidebar render and
 * the keyboard walker so they never drift.
 */
function visibleByGroup<T extends { id: string }>(
  items: T[],
  groups: readonly MailboxGroup[],
  collapsedGroupIds: ReadonlySet<string>,
): T[] {
  const partition = partitionByGroup(items, groups)
  const visible = [...partition.ungrouped]
  for (const entry of partition.groups) {
    if (collapsedGroupIds.has(entry.group.id)) continue
    visible.push(...entry.mailboxes)
  }
  return visible
}

export function visibleSourceMailboxes(
  mailboxes: Mailbox[],
  groups: readonly MailboxGroup[],
  collapsedGroupIds: ReadonlySet<string>,
): Mailbox[] {
  return visibleByGroup(mailboxes, groups, collapsedGroupIds)
}

export function visibleSmartMailboxes(
  smartMailboxes: SmartMailboxSummary[],
  groups: readonly MailboxGroup[],
  collapsedGroupIds: ReadonlySet<string>,
): SmartMailboxSummary[] {
  return visibleByGroup(smartMailboxes, groups, collapsedGroupIds)
}

/**
 * The subset of `groups` a SMART mailbox may be assigned to without creating a
 * mixed (smart + source) group: a group is offerable only when EVERY current
 * member is a smart mailbox. A source-populated or mixed group fails this test
 * (its source ids are absent from the smart-id set), so it is never offered in a
 * smart mailbox's "Add to group" menu. The symmetric guard on the source side
 * holds implicitly: a smart-populated group never matches a source's own mailbox
 * ids, so it never surfaces there. Empty groups don't exist (they're pruned), so
 * every returned group is genuinely smart-homogeneous.
 */
export function smartAssignableGroups(
  groups: readonly MailboxGroup[],
  smartMailboxIds: ReadonlySet<string>,
): MailboxGroup[] {
  return groups.filter(
    (group) =>
      group.mailboxIds.length > 0 &&
      group.mailboxIds.every((id) => smartMailboxIds.has(id)),
  )
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

/** Member-id -> group-id lookup over a partition's groups — the "which group
 *  is this row in" answer behind the Add-to/Remove-from-group menu items.
 *  Shared by the Smart section and SourceSection so they never drift. */
export function groupIdByMailbox<T extends { id: string }>(
  groups: readonly MailboxGroupEntry<T>[],
): Map<string, string> {
  const map = new Map<string, string>()
  for (const entry of groups) {
    for (const mailbox of entry.mailboxes) {
      map.set(mailbox.id, entry.group.id)
    }
  }
  return map
}
