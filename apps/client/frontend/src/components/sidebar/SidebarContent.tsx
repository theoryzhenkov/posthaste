import { useMemo } from 'react'
import { AlertCircle } from 'lucide-react'

import type { SmartMailboxSummary } from '@/data/transport/api'
import { smartMailboxAccent } from '@/domain/role'
import type { useMailboxNavigationReadModels } from '@/data/models/mailboxNavigation'

import type { SidebarSelection } from '@/data/models/selection'
import { SortableList, SortableRow } from '../ui/display/SortableList'
import { groupIdByMailbox,
  fallbackAccountAppearance,
  partitionSmartMailboxes,
  smartAssignableGroups,
} from './model'
import { GroupHeader, SectionHeader, SourceSection } from './SourceSection'
import { SmartMailboxItem } from './SidebarItems'
import { useMailboxGroups, useMailboxGroupMutations } from './hooks/useMailboxGroups'

type NavigationReadModels = ReturnType<typeof useMailboxNavigationReadModels>

type SourceReadModel = NavigationReadModels['sources'][number]

export function SidebarLoading() {
  return (
    <div className="space-y-3 px-1 py-1">
      {Array.from({ length: 5 }).map((_, i) => (
        <div key={i} className="flex items-center gap-2 py-1.5">
          <div className="h-4 w-4 animate-pulse rounded-[4px] bg-muted" />
          <div
            className="h-3 animate-pulse rounded bg-muted"
            style={{ width: `${60 + ((i * 17) % 30)}%` }}
          />
        </div>
      ))}
    </div>
  )
}

export function SidebarError({ onRetry }: { onRetry: () => void }) {
  return (
    <div className="px-3 py-4">
      <div className="flex flex-col items-center gap-2 text-center">
        <AlertCircle size={20} className="text-destructive/60" />
        <p className="text-xs text-destructive">Failed to load sidebar</p>
        <button
          type="button"
          className="text-xs text-muted-foreground underline underline-offset-2 hover:text-foreground"
          onClick={onRetry}
        >
          Try again
        </button>
      </div>
    </div>
  )
}

export function SmartMailboxSection({
  collapsed,
  mailboxes,
  selectedView,
  isPaneActive,
  collapsedGroupIds,
  onOpenSmartMailboxSettings,
  onSelectSmartMailbox,
  onReorder,
  onToggle,
  onToggleGroupCollapsed,
}: {
  collapsed: boolean
  mailboxes: SmartMailboxSummary[]
  selectedView: SidebarSelection | null
  isPaneActive: boolean
  /** Collapsed sidebar Group ids (shared with the j/k walker in Sidebar). */
  collapsedGroupIds: ReadonlySet<string>
  onOpenSmartMailboxSettings: (smartMailboxId: string) => void
  onSelectSmartMailbox: (smartMailboxId: string, name: string) => void
  onReorder: (orderedIds: string[]) => void
  onToggle: () => void
  onToggleGroupCollapsed: (groupId: string) => void
}) {
  const groups = useMailboxGroups()
  const groupMutations = useMailboxGroupMutations()
  // Partition the smart mailboxes into synced Groups + an ungrouped remainder,
  // mirroring SourceSection. A group surfaces here only if it holds ≥1 SMART
  // mailbox; ungrouped smart mailboxes render flat (and stay drag-reorderable).
  const partition = useMemo(
    () => partitionSmartMailboxes(mailboxes, groups),
    [mailboxes, groups],
  )
  // HOMOGENEITY: the "Add to group" list for a smart mailbox offers ONLY groups
  // whose every member is a smart mailbox (never a source-populated group), so a
  // group can never become mixed.
  const smartIds = useMemo(
    () => new Set(mailboxes.map((mailbox) => mailbox.id)),
    [mailboxes],
  )
  const assignableGroups = useMemo(
    () => smartAssignableGroups(groups, smartIds),
    [groups, smartIds],
  )
  const currentGroupIdByMailbox = useMemo(
    () => groupIdByMailbox(partition.groups),
    [partition.groups],
  )

  const renderSmartMailboxItem = (
    smartMailbox: SmartMailboxSummary,
    depth = 0,
  ) => (
    <SmartMailboxItem
      id={smartMailbox.id}
      name={smartMailbox.name}
      role={smartMailbox.role}
      defaultKey={smartMailbox.defaultKey}
      unreadMessages={smartMailbox.unreadMessages}
      accent={smartMailboxAccent(smartMailbox.role, smartMailbox.name)}
      depth={depth}
      groups={assignableGroups}
      currentGroupId={currentGroupIdByMailbox.get(smartMailbox.id) ?? null}
      isSelected={
        selectedView?.kind === 'smart-mailbox' &&
        selectedView.id === smartMailbox.id
      }
      isPaneActive={isPaneActive}
      onSelect={() => onSelectSmartMailbox(smartMailbox.id, smartMailbox.name)}
      onOpenSettings={onOpenSmartMailboxSettings}
      onAssignToGroup={groupMutations.assignToGroup}
      onRemoveFromGroup={groupMutations.removeFromGroup}
      onCreateGroup={groupMutations.createGroup}
    />
  )

  return (
    <>
      <SectionHeader label="Smart" collapsed={collapsed} onToggle={onToggle} />
      {!collapsed && (
        <div className="space-y-0.5 py-1">
          <SortableList
            ids={partition.ungrouped.map((mailbox) => mailbox.id)}
            onReorder={onReorder}
          >
            {partition.ungrouped.map((smartMailbox) => (
              <SortableRow key={smartMailbox.id} id={smartMailbox.id}>
                {renderSmartMailboxItem(smartMailbox)}
              </SortableRow>
            ))}
          </SortableList>
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
                    {entry.mailboxes.map((smartMailbox) => (
                      <div key={smartMailbox.id}>
                        {renderSmartMailboxItem(smartMailbox, 1)}
                      </div>
                    ))}
                  </div>
                )}
              </div>
            )
          })}
        </div>
      )}
    </>
  )
}

export function AccountsSection({
  collapsed,
  selectedView,
  isPaneActive,
  collapsedSourceIds,
  collapsedGroupIds,
  sources,
  onOpenAccountSettings,
  onSelectSourceMailbox,
  onSyncSource,
  onReorder,
  onToggle,
  onToggleSourceCollapsed,
  onToggleGroupCollapsed,
}: {
  collapsed: boolean
  selectedView: SidebarSelection | null
  isPaneActive: boolean
  collapsedSourceIds: ReadonlySet<string>
  collapsedGroupIds: ReadonlySet<string>
  sources: SourceReadModel[]
  onOpenAccountSettings: (sourceId: string) => void
  onSelectSourceMailbox: (
    sourceId: string,
    mailboxId: string,
    name: string,
  ) => void
  onSyncSource: (sourceId: string) => void
  onReorder: (orderedIds: string[]) => void
  onToggle: () => void
  onToggleSourceCollapsed: (sourceId: string) => void
  onToggleGroupCollapsed: (groupId: string) => void
}) {
  return (
    <>
      <SectionHeader
        label="Accounts"
        collapsed={collapsed}
        onToggle={onToggle}
      />
      {!collapsed && (
        <div className="space-y-2 py-1">
          <SortableList
            ids={sources.map((source) => source.id)}
            onReorder={onReorder}
          >
            {sources.map((source) => (
              <SortableRow key={source.id} id={source.id}>
                <SourceSection
                  source={source}
                  appearance={
                    source.appearance ??
                    fallbackAccountAppearance(source.id, source.name)
                  }
                  selectedView={selectedView}
                  isPaneActive={isPaneActive}
                  collapsed={collapsedSourceIds.has(source.id)}
                  collapsedGroupIds={collapsedGroupIds}
                  onToggleCollapsed={() => onToggleSourceCollapsed(source.id)}
                  onToggleGroupCollapsed={onToggleGroupCollapsed}
                  onOpenAccountSettings={onOpenAccountSettings}
                  onSelectSourceMailbox={onSelectSourceMailbox}
                  onSyncSource={onSyncSource}
                />
              </SortableRow>
            ))}
          </SortableList>
        </div>
      )}
    </>
  )
}
