import { AlertCircle } from 'lucide-react'

import type { SmartMailboxSummary, TagSummary } from '@/api/types'
import { smartMailboxAccent } from '@/mailboxRoles'
import type { useMailboxNavigationReadModels } from '@/mailboxNavigationReadModels'

import type { SidebarSelection } from '../Sidebar'
import { fallbackAccountAppearance } from './model'
import { SectionHeader, SourceSection } from './SourceSection'
import { SmartMailboxItem, TagItem } from './SidebarItems'

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
  onOpenSmartMailboxSettings,
  onSelectSmartMailbox,
  onToggle,
}: {
  collapsed: boolean
  mailboxes: SmartMailboxSummary[]
  selectedView: SidebarSelection | null
  onOpenSmartMailboxSettings: (smartMailboxId: string) => void
  onSelectSmartMailbox: (smartMailboxId: string, name: string) => void
  onToggle: () => void
}) {
  return (
    <>
      <SectionHeader label="Smart" collapsed={collapsed} onToggle={onToggle} />
      {!collapsed && (
        <div className="space-y-0.5 py-1">
          {mailboxes.map((smartMailbox) => (
            <SmartMailboxItem
              key={smartMailbox.id}
              id={smartMailbox.id}
              name={smartMailbox.name}
              role={smartMailbox.role}
              defaultKey={smartMailbox.defaultKey}
              unreadMessages={smartMailbox.unreadMessages}
              accent={smartMailboxAccent(smartMailbox.role, smartMailbox.name)}
              isSelected={
                selectedView?.kind === 'smart-mailbox' &&
                selectedView.id === smartMailbox.id
              }
              onSelect={() =>
                onSelectSmartMailbox(smartMailbox.id, smartMailbox.name)
              }
              onOpenSettings={onOpenSmartMailboxSettings}
            />
          ))}
        </div>
      )}
    </>
  )
}

export function TagsSection({
  tags,
  onSelectTag,
}: {
  tags: TagSummary[]
  onSelectTag: (tag: string) => void
}) {
  if (tags.length === 0) {
    return null
  }
  return (
    <>
      <SectionHeader label="Tags" collapsed={false} onToggle={() => {}} />
      <div className="space-y-0.5 py-1">
        {tags.map((tag) => (
          <TagItem
            key={tag.name}
            tag={tag}
            onSelect={() => onSelectTag(tag.name)}
          />
        ))}
      </div>
    </>
  )
}

export function AccountsSection({
  collapsed,
  selectedView,
  sources,
  onOpenAccountSettings,
  onSelectSourceMailbox,
  onSyncSource,
  onToggle,
}: {
  collapsed: boolean
  selectedView: SidebarSelection | null
  sources: SourceReadModel[]
  onOpenAccountSettings: (sourceId: string) => void
  onSelectSourceMailbox: (
    sourceId: string,
    mailboxId: string,
    name: string,
  ) => void
  onSyncSource: (sourceId: string) => void
  onToggle: () => void
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
          {sources.map((source) => (
            <SourceSection
              key={source.id}
              source={source}
              appearance={
                source.appearance ??
                fallbackAccountAppearance(source.id, source.name)
              }
              selectedView={selectedView}
              onOpenAccountSettings={onOpenAccountSettings}
              onSelectSourceMailbox={onSelectSourceMailbox}
              onSyncSource={onSyncSource}
            />
          ))}
        </div>
      )}
    </>
  )
}
