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

/** One rendered Group within the Smart section: the group and its member smart
 *  mailboxes, in the section's own (resolved) smart-mailbox order. */
export interface SmartMailboxGroupEntry {
  group: MailboxGroup
  mailboxes: SmartMailboxSummary[]
}

/** The partition of the smart mailboxes into client-side Groups + an ungrouped
 *  remainder. Presentation only. Mirrors {@link PartitionedSourceMailboxes}. */
export interface PartitionedSmartMailboxes {
  /** Smart mailboxes belonging to no group, in their original order. */
  ungrouped: SmartMailboxSummary[]
  /** Groups that hold at least one SMART mailbox, ordered by `group.order`
   *  (ties broken by name for stability). */
  groups: SmartMailboxGroupEntry[]
}

/**
 * Partition the smart mailboxes into Groups + ungrouped remainder, driven by the
 * SAME synced `mailboxGroups` setting used for source mailboxes. A Group
 * surfaces in the Smart section only when it holds ≥1 smart mailbox, and each
 * group's members are FILTERED to the smart-mailbox id set — so a stray mixed
 * group (smart + source ids) shows only its smart members here, and a purely
 * source group (no smart members) is dropped from this section entirely. This is
 * what keeps a source group out of the Smart section: group ids share one
 * namespace, but membership is homogeneous per the UI's enforcement.
 *
 * Mirrors {@link partitionSourceMailboxes}: iterating over `smartMailboxes` and
 * grouping by id is itself the smart-set filter (a non-smart member id never
 * matches a smart mailbox, so it contributes nothing here).
 */
export function partitionSmartMailboxes(
  smartMailboxes: SmartMailboxSummary[],
  groups: readonly MailboxGroup[],
): PartitionedSmartMailboxes {
  const groupByMailboxId = new Map<string, string>()
  for (const group of groups) {
    for (const mailboxId of group.mailboxIds) {
      groupByMailboxId.set(mailboxId, group.id)
    }
  }

  const ungrouped: SmartMailboxSummary[] = []
  const membersByGroupId = new Map<string, SmartMailboxSummary[]>()
  for (const mailbox of smartMailboxes) {
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
 * The smart mailboxes in `j`/`k` walk order, honoring Group collapse: every
 * ungrouped smart mailbox, then each expanded Group's members. A collapsed Group
 * contributes nothing. Mirrors {@link visibleSourceMailboxes} so the Smart
 * section render and the keyboard walker never drift.
 */
export function visibleSmartMailboxes(
  smartMailboxes: SmartMailboxSummary[],
  groups: readonly MailboxGroup[],
  collapsedGroupIds: ReadonlySet<string>,
): SmartMailboxSummary[] {
  const partition = partitionSmartMailboxes(smartMailboxes, groups)
  const visible = [...partition.ungrouped]
  for (const entry of partition.groups) {
    if (collapsedGroupIds.has(entry.group.id)) continue
    visible.push(...entry.mailboxes)
  }
  return visible
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
