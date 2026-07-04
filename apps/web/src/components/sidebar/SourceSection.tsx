import { useMemo } from 'react'
import { ChevronDown, ChevronRight, RefreshCw, Settings } from 'lucide-react'

import type { AccountAppearance, Mailbox } from '@/api/types'
import { useMailboxColorLookup } from '@/hooks/useMailboxColors'
import { useMailboxCounts } from '@/live-store/store'

import { AccountMark } from '../AccountMark'
import type { SidebarSelection } from '../Sidebar'
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from '../ui/context-menu'
import { MailboxItem } from './SidebarItems'

export function SourceSection({
  source,
  appearance,
  selectedView,
  isPaneActive,
  collapsed,
  onToggleCollapsed,
  onOpenAccountSettings,
  onSelectSourceMailbox,
  onSyncSource,
}: {
  source: {
    id: string
    name: string
    mailboxes: Mailbox[]
  }
  appearance: AccountAppearance
  selectedView: SidebarSelection | null
  /** Whether the sidebar is the focused pane (drives accent-vs-grey selection). */
  isPaneActive: boolean
  collapsed: boolean
  onToggleCollapsed: () => void
  onOpenAccountSettings: (sourceId: string) => void
  onSelectSourceMailbox: (
    sourceId: string,
    mailboxId: string,
    name: string,
  ) => void
  onSyncSource: (sourceId: string) => void
}) {
  const mailboxColorHue = useMailboxColorLookup()
  // The account header's aggregate unread reflects live COUNTS (D116): sum each
  // mailbox's live count, falling back to the query's server count when no frame
  // has seeded a live entry yet (bootstrap).
  const liveCounts = useMailboxCounts(source.id)
  const unreadTotal = useMemo(
    () =>
      source.mailboxes.reduce((sum, mailbox) => {
        const live = liveCounts[mailbox.id]
        return sum + (live ? live.unread : mailbox.unreadEmails)
      }, 0),
    [source.mailboxes, liveCounts],
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
      {!collapsed && (
        <div className="space-y-0.5">
          {source.mailboxes.map((mailbox) => (
            <MailboxItem
              key={`${source.id}:${mailbox.id}`}
              sourceId={source.id}
              sourceName={source.name}
              mailbox={mailbox}
              colorHue={mailboxColorHue(source.id, mailbox.id)}
              depth={1}
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
            />
          ))}
        </div>
      )}
    </div>
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
