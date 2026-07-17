import { useMemo, useState } from 'react'
import {
  AlertTriangle,
  ChevronDown,
  ChevronRight,
  Edit3,
  Folder,
  FolderPlus,
  RefreshCw,
  Settings,
  Trash2,
} from 'lucide-react'

import type { AccountHealth } from '@/accountHealth'
import type { AccountAppearance, Mailbox, MailboxGroup } from '@/api/types'
import { useMailboxColorLookup } from '@/hooks/useMailboxColors'

import { AccountMark } from '../AccountMark'
import type { SidebarSelection } from '../Sidebar'
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from '../ui/context-menu'
import { GroupNameDialog } from './GroupNameDialog'
import { partitionSourceMailboxes } from './model'
import { MailboxItem } from './SidebarItems'
import { useMailboxGroups, useMailboxGroupMutations } from './useMailboxGroups'
import { NewMailboxDialog } from './NewMailboxDialog'

export function SourceSection({
  source,
  appearance,
  selectedView,
  isPaneActive,
  collapsed,
  collapsedGroupIds,
  onToggleCollapsed,
  onToggleGroupCollapsed,
  onOpenAccountSettings,
  onSelectSourceMailbox,
  onSyncSource,
}: {
  source: {
    id: string
    name: string
    mailboxes: Mailbox[]
    health?: AccountHealth
  }
  appearance: AccountAppearance
  selectedView: SidebarSelection | null
  /** Whether the sidebar is the focused pane (drives accent-vs-grey selection). */
  isPaneActive: boolean
  collapsed: boolean
  /** Collapsed sidebar Group ids (shared with the j/k walker in Sidebar). */
  collapsedGroupIds: ReadonlySet<string>
  onToggleCollapsed: () => void
  onToggleGroupCollapsed: (groupId: string) => void
  onOpenAccountSettings: (sourceId: string) => void
  onSelectSourceMailbox: (
    sourceId: string,
    mailboxId: string,
    name: string,
  ) => void
  onSyncSource: (sourceId: string) => void
}) {
  const mailboxColorHue = useMailboxColorLookup()
  const groups = useMailboxGroups()
  const groupMutations = useMailboxGroupMutations()
  const [isNewMailboxOpen, setIsNewMailboxOpen] = useState(false)
  // Partition THIS source's mailboxes into synced Groups + an ungrouped
  // remainder. A group surfaces here only if it holds ≥1 of this source's
  // mailboxes; ungrouped mailboxes render flat as before.
  const partition = useMemo(
    () => partitionSourceMailboxes(source.mailboxes, groups),
    [source.mailboxes, groups],
  )
  // The group each of this source's mailboxes belongs to (for the "Add to group"
  // check-mark + "Remove from group" item), and the list of this source's groups
  // for the submenu.
  const sourceGroups = useMemo(
    () => partition.groups.map((entry) => entry.group),
    [partition.groups],
  )
  const currentGroupIdByMailbox = useMemo(() => {
    const map = new Map<string, string>()
    for (const entry of partition.groups) {
      for (const mailbox of entry.mailboxes) {
        map.set(mailbox.id, entry.group.id)
      }
    }
    return map
  }, [partition.groups])
  // The account header's aggregate unread sums the react-query mailbox rows'
  // counts: invalidation + the optimistic overlay keep `unreadEmails` live,
  // so no separate live-count source exists.
  const unreadTotal = useMemo(
    () =>
      source.mailboxes.reduce((sum, mailbox) => sum + mailbox.unreadEmails, 0),
    [source.mailboxes],
  )

  const headerButton = (
    <button
      type="button"
      className="ph-focus-ring mx-1.5 mt-1 flex h-[30px] w-[calc(100%-0.75rem)] items-center gap-2 rounded-[5px] px-2 text-left transition-colors hover:bg-[var(--sidebar-accent)]"
      onClick={onToggleCollapsed}
    >
      {collapsed ? (
        <ChevronRight
          size={12}
          strokeWidth={1.5}
          className="text-muted-foreground"
        />
      ) : (
        <ChevronDown
          size={12}
          strokeWidth={1.5}
          className="text-muted-foreground"
        />
      )}
      <AccountMark
        appearance={appearance}
        className="size-[18px] text-[10px]"
      />
      <span className="min-w-0 flex-1 truncate text-[13px] font-semibold text-sidebar-foreground">
        {source.name}
      </span>
      {source.health?.isUnhealthy && (
        <span
          role="img"
          aria-label={source.health.message ?? source.health.label}
          title={source.health.message ?? source.health.label}
          className="shrink-0"
        >
          <AlertTriangle
            size={13}
            strokeWidth={1.8}
            className={
              source.health.severity === 'error'
                ? 'text-rose-500'
                : 'text-amber-500'
            }
          />
        </span>
      )}
      {unreadTotal > 0 && (
        <span className="rounded-[4px] bg-signal-unread px-1.5 font-mono text-[11px] font-semibold tabular-nums text-white">
          {unreadTotal}
        </span>
      )}
    </button>
  )

  return (
    <div>
      <ContextMenu>
        <ContextMenuTrigger asChild>{headerButton}</ContextMenuTrigger>
        <ContextMenuContent className="min-w-48">
          <ContextMenuItem onSelect={onToggleCollapsed}>
            {collapsed ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
            {collapsed ? 'Expand' : 'Collapse'}
          </ContextMenuItem>
          <ContextMenuSeparator />
          <ContextMenuItem onSelect={() => setIsNewMailboxOpen(true)}>
            <FolderPlus size={14} />
            New mailbox
          </ContextMenuItem>
          <ContextMenuItem onSelect={() => onSyncSource(source.id)}>
            <RefreshCw size={14} />
            Sync account
          </ContextMenuItem>
          <ContextMenuItem onSelect={() => onOpenAccountSettings(source.id)}>
            <Settings size={14} />
            Account settings
          </ContextMenuItem>
        </ContextMenuContent>
      </ContextMenu>
      <NewMailboxDialog
        sourceId={source.id}
        open={isNewMailboxOpen}
        onOpenChange={setIsNewMailboxOpen}
      />
      {!collapsed && (
        <div className="space-y-0.5">
          {partition.ungrouped.map((mailbox) => renderMailboxItem(mailbox, 1))}
          {partition.groups.map((entry) => {
            const groupCollapsed = collapsedGroupIds.has(entry.group.id)
            return (
              <div key={entry.group.id}>
                <GroupHeader
                  group={entry.group}
                  collapsed={groupCollapsed}
                  onToggleCollapsed={() =>
                    onToggleGroupCollapsed(entry.group.id)
                  }
                  onRename={(name) =>
                    groupMutations.renameGroup(entry.group.id, name)
                  }
                  onDelete={() => groupMutations.deleteGroup(entry.group.id)}
                />
                {!groupCollapsed && (
                  <div className="space-y-0.5">
                    {entry.mailboxes.map((mailbox) =>
                      renderMailboxItem(mailbox, 2),
                    )}
                  </div>
                )}
              </div>
            )
          })}
        </div>
      )}
    </div>
  )

  function renderMailboxItem(mailbox: Mailbox, depth: number) {
    return (
      <MailboxItem
        key={`${source.id}:${mailbox.id}`}
        sourceId={source.id}
        sourceName={source.name}
        mailbox={mailbox}
        colorHue={mailboxColorHue(source.id, mailbox.id)}
        depth={depth}
        groups={sourceGroups}
        currentGroupId={currentGroupIdByMailbox.get(mailbox.id) ?? null}
        onOpenAccountSettings={onOpenAccountSettings}
        isSelected={
          selectedView?.kind === 'source-mailbox' &&
          selectedView.sourceId === source.id &&
          selectedView.mailboxId === mailbox.id
        }
        isPaneActive={isPaneActive}
        onSelect={() =>
          onSelectSourceMailbox(
            source.id,
            mailbox.id,
            `${source.name} / ${mailbox.name}`,
          )
        }
        onSyncSource={onSyncSource}
        onAssignToGroup={groupMutations.assignToGroup}
        onRemoveFromGroup={groupMutations.removeFromGroup}
        onCreateGroup={groupMutations.createGroup}
      />
    )
  }
}

/**
 * A collapsible sidebar Group header (presentation only). Clicking toggles the
 * group's collapse; a context menu offers rename / delete. Delete only ungroups
 * the members — it never touches mailboxes or mail.
 *
 */
export function GroupHeader({
  group,
  collapsed,
  onToggleCollapsed,
  onRename,
  onDelete,
}: {
  group: MailboxGroup
  collapsed: boolean
  onToggleCollapsed: () => void
  onRename: (name: string) => void
  onDelete: () => void
}) {
  const [isRenameOpen, setIsRenameOpen] = useState(false)
  const headerButton = (
    <button
      type="button"
      aria-expanded={!collapsed}
      className="ph-focus-ring mx-1.5 flex h-[var(--density-sidebar-row-height)] w-[calc(100%-0.75rem)] items-center gap-2 rounded-[5px] pl-[22px] pr-2 text-left text-[13px] font-medium text-sidebar-foreground/92 transition-colors hover:bg-[var(--sidebar-accent)]"
      onClick={onToggleCollapsed}
    >
      {collapsed ? (
        <ChevronRight
          size={12}
          strokeWidth={1.5}
          className="shrink-0 text-muted-foreground"
        />
      ) : (
        <ChevronDown
          size={12}
          strokeWidth={1.5}
          className="shrink-0 text-muted-foreground"
        />
      )}
      <Folder size={14} className="shrink-0 text-muted-foreground" />
      <span className="min-w-0 flex-1 truncate">{group.name}</span>
    </button>
  )

  return (
    <>
      <ContextMenu>
        <ContextMenuTrigger asChild>{headerButton}</ContextMenuTrigger>
        <ContextMenuContent className="min-w-44">
          <ContextMenuItem onSelect={() => setIsRenameOpen(true)}>
            <Edit3 size={14} />
            Rename group
          </ContextMenuItem>
          <ContextMenuSeparator />
          <ContextMenuItem variant="destructive" onSelect={onDelete}>
            <Trash2 size={14} />
            Delete group
          </ContextMenuItem>
        </ContextMenuContent>
      </ContextMenu>
      <GroupNameDialog
        mode="rename"
        initialName={group.name}
        open={isRenameOpen}
        onOpenChange={setIsRenameOpen}
        onSubmit={onRename}
      />
    </>
  )
}

export function SectionHeader({
  label,
  collapsed,
  onToggle,
}: {
  label: string
  collapsed: boolean
  onToggle: () => void
}) {
  return (
    <button
      type="button"
      className="ph-focus-ring flex h-7 w-full items-center px-3 text-left font-mono text-[11px] font-semibold uppercase tracking-[0.06em] text-[var(--sidebar-section-label)] transition-colors hover:text-sidebar-foreground"
      onClick={onToggle}
      aria-expanded={!collapsed}
    >
      <span>{label}</span>
    </button>
  )
}
