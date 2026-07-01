import { AlertCircle } from 'lucide-react'

import type { SmartMailboxSummary } from '@/api/types'
import { smartMailboxAccent } from '@/mailboxRoles'
import type { useMailboxNavigationReadModels } from '@/mailboxNavigationReadModels'

import type { SidebarSelection } from '../Sidebar'
import { SortableList, SortableRow } from '../ui/SortableList'
import { fallbackAccountAppearance } from './model'
import { SectionHeader, SourceSection } from './SourceSection'
import { SmartMailboxItem } from './SidebarItems'

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
  onOpenSmartMailboxSettings,
  onSelectSmartMailbox,
  onReorder,
  onToggle,
}: {
  collapsed: boolean
  mailboxes: SmartMailboxSummary[]
  selectedView: SidebarSelection | null
  isPaneActive: boolean
  onOpenSmartMailboxSettings: (smartMailboxId: string) => void
  onSelectSmartMailbox: (smartMailboxId: string, name: string) => void
  onReorder: (orderedIds: string[]) => void
  onToggle: () => void
}) {
  return (
    <>
      <SectionHeader label="Smart" collapsed={collapsed} onToggle={onToggle} />
      {!collapsed && (
        <div className="space-y-0.5 py-1">
          <SortableList
            ids={mailboxes.map((mailbox) => mailbox.id)}
            onReorder={onReorder}
          >
            {mailboxes.map((smartMailbox) => (
              <SortableRow key={smartMailbox.id} id={smartMailbox.id}>
                <SmartMailboxItem
                  id={smartMailbox.id}
                  name={smartMailbox.name}
                  role={smartMailbox.role}
                  defaultKey={smartMailbox.defaultKey}
                  unreadMessages={smartMailbox.unreadMessages}
                  accent={smartMailboxAccent(
                    smartMailbox.role,
                    smartMailbox.name,
                  )}
                  isSelected={
                    selectedView?.kind === 'smart-mailbox' &&
                    selectedView.id === smartMailbox.id
                  }
                  isPaneActive={isPaneActive}
                  onSelect={() =>
                    onSelectSmartMailbox(smartMailbox.id, smartMailbox.name)
                  }
                  onOpenSettings={onOpenSmartMailboxSettings}
                />
              </SortableRow>
            ))}
          </SortableList>
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
  sources,
  onOpenAccountSettings,
  onSelectSourceMailbox,
  onSyncSource,
  onReorder,
  onToggle,
  onToggleSourceCollapsed,
}: {
  collapsed: boolean
  selectedView: SidebarSelection | null
  isPaneActive: boolean
  collapsedSourceIds: ReadonlySet<string>
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
                  onToggleCollapsed={() => onToggleSourceCollapsed(source.id)}
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
