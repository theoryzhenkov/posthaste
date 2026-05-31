/**
 * Left-pane sidebar with smart mailbox and source mailbox navigation.
 *
 * Renders normalized account, mailbox, smart mailbox, and tag read models.
 *
 * @spec docs/L1-ui#component-hierarchy
 * @spec docs/L0-ui#navigation-model
 */
import { useMemo, useState } from 'react'
import {
  AlertCircle,
  ChevronDown,
  ChevronRight,
  Edit3,
  MailOpen,
  RefreshCw,
  Settings,
} from 'lucide-react'
import type {
  AccountAppearance,
  Mailbox,
  SmartMailboxSummary,
  TagSummary,
} from '../api/types'
import { cn } from '../lib/utils'
import {
  mailboxRoleAccent,
  mailboxRoleFromName,
  renderMailboxRoleIcon,
  smartMailboxAccent,
  smartMailboxFallbackIcon,
} from '../mailboxRoles'
import { useMailboxNavigationReadModels } from '../mailboxNavigationReadModels'
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from './ui/context-menu'
import { AccountMark } from './AccountMark'

/**
 * Discriminated union representing the current sidebar selection.
 * @spec docs/L0-ui#navigation-model
 */
export type SidebarSelection =
  | { kind: 'smart-mailbox'; id: string; name: string }
  | {
      kind: 'source-mailbox'
      sourceId: string
      mailboxId: string
      name: string
    }

/** @spec docs/L1-ui#component-hierarchy */
interface SidebarProps {
  selectedView: SidebarSelection | null
  onOpenAccountSettings: (sourceId: string) => void
  onOpenSmartMailboxSettings: (smartMailboxId: string) => void
  onSelectSmartMailbox: (smartMailboxId: string, name: string) => void
  onSelectSourceMailbox: (
    sourceId: string,
    mailboxId: string,
    name: string,
  ) => void
  onSelectTag: (tag: string) => void
  onSyncSource: (sourceId: string) => void
}

function roleIcon(role: Mailbox['role'], size = 14): React.ReactNode {
  return renderMailboxRoleIcon(role, size)
}

function fallbackAccountAppearance(
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

/** Icon for smart mailboxes based on the name heuristic. */
function smartMailboxIcon(name: string, size = 14): React.ReactNode {
  return renderMailboxRoleIcon(
    mailboxRoleFromName(name),
    size,
    smartMailboxFallbackIcon(name),
  )
}

function smartMailboxPriority(name: string): number {
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

function displaySmartMailboxName(name: string): string {
  return name.trim().toLowerCase() === 'inbox' ? 'All Inboxes' : name
}

function partitionSmartMailboxes(smartMailboxes: SmartMailboxSummary[]) {
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

function itemButtonClass(isSelected: boolean, depth = 0): string {
  return cn(
    'mx-1.5 flex h-[28px] w-[calc(100%-0.75rem)] items-center gap-2 rounded-[5px] pr-2 text-left text-[13px] font-medium transition-colors',
    'ph-focus-ring hover:bg-[var(--sidebar-accent)]',
    isSelected &&
      'bg-[var(--list-selection)] text-[var(--list-selection-foreground)]',
    !isSelected && 'text-sidebar-foreground/92',
    depth > 0 ? 'pl-[22px]' : 'pl-2',
  )
}

/** Smart mailbox row with unread badge. */
function ViewItem({
  id,
  name,
  unreadMessages,
  accent,
  isSelected,
  onOpenSettings,
  onSelect,
}: {
  id: string
  name: string
  unreadMessages?: number
  accent?: string
  isSelected: boolean
  onOpenSettings: (smartMailboxId: string) => void
  onSelect: () => void
}) {
  const button = (
    <button
      className={itemButtonClass(isSelected)}
      onClick={onSelect}
      onContextMenu={onSelect}
      type="button"
    >
      <span
        className="flex w-4 justify-center"
        style={accent ? { color: accent } : undefined}
      >
        {smartMailboxIcon(name)}
      </span>
      <span className="min-w-0 flex-1 truncate">
        {displaySmartMailboxName(name)}
      </span>
      {unreadMessages != null && unreadMessages > 0 && (
        <span
          className={cn(
            'font-mono text-[11px] font-medium tabular-nums',
            isSelected
              ? 'text-[var(--list-selection-foreground)]'
              : 'text-muted-foreground/80',
          )}
        >
          {unreadMessages}
        </span>
      )}
    </button>
  )

  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>{button}</ContextMenuTrigger>
      <ContextMenuContent className="min-w-44">
        <ContextMenuItem onSelect={onSelect}>
          <MailOpen size={14} />
          Open
        </ContextMenuItem>
        <ContextMenuSeparator />
        <ContextMenuItem onSelect={() => onOpenSettings(id)}>
          <Edit3 size={14} />
          Edit mailbox
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  )
}

/** Tag row with unread badge. */
function TagItem({ tag, onSelect }: { tag: TagSummary; onSelect: () => void }) {
  return (
    <button className={itemButtonClass(false)} onClick={onSelect} type="button">
      <span
        className="flex w-4 justify-center"
        style={{ color: smartMailboxAccent(tag.name) }}
      >
        {smartMailboxIcon(tag.name)}
      </span>
      <span className="min-w-0 flex-1 truncate">{tag.name}</span>
      {tag.unreadMessages > 0 && (
        <span className="font-mono text-[11px] font-medium tabular-nums text-muted-foreground/80">
          {tag.unreadMessages}
        </span>
      )}
    </button>
  )
}

/** Source mailbox row with role icon and unread badge. */
function MailboxItem({
  sourceId,
  sourceName,
  mailbox,
  isSelected,
  depth = 0,
  onOpenAccountSettings,
  onSelect,
  onSyncSource,
}: {
  sourceId: string
  sourceName: string
  mailbox: Mailbox
  isSelected: boolean
  depth?: number
  onOpenAccountSettings: (sourceId: string) => void
  onSelect: () => void
  onSyncSource: (sourceId: string) => void
}) {
  const button = (
    <button
      className={itemButtonClass(isSelected, depth)}
      onClick={onSelect}
      onContextMenu={onSelect}
      type="button"
    >
      <span
        className="flex w-4 justify-center"
        style={{ color: mailboxRoleAccent(mailbox.role) }}
      >
        {roleIcon(mailbox.role)}
      </span>
      <span className="min-w-0 flex-1 truncate">{mailbox.name}</span>
      {mailbox.unreadEmails > 0 && (
        <span
          className={cn(
            'font-mono text-[11px] font-medium tabular-nums',
            isSelected
              ? 'text-[var(--list-selection-foreground)]'
              : 'text-muted-foreground/80',
          )}
        >
          {mailbox.unreadEmails}
        </span>
      )}
    </button>
  )

  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>{button}</ContextMenuTrigger>
      <ContextMenuContent className="min-w-48">
        <ContextMenuItem onSelect={onSelect}>
          <MailOpen size={14} />
          Open mailbox
        </ContextMenuItem>
        <ContextMenuSeparator />
        <ContextMenuItem onSelect={() => onSyncSource(sourceId)}>
          <RefreshCw size={14} />
          Sync {sourceName}
        </ContextMenuItem>
        <ContextMenuItem onSelect={() => onOpenAccountSettings(sourceId)}>
          <Settings size={14} />
          Account settings
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  )
}

/** Collapsible source section with its mailbox children. */
function SourceSection({
  source,
  appearance,
  selectedView,
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
  onOpenAccountSettings: (sourceId: string) => void
  onSelectSourceMailbox: (
    sourceId: string,
    mailboxId: string,
    name: string,
  ) => void
  onSyncSource: (sourceId: string) => void
}) {
  const [collapsed, setCollapsed] = useState(false)
  const unreadTotal = useMemo(
    () =>
      source.mailboxes.reduce((sum, mailbox) => sum + mailbox.unreadEmails, 0),
    [source.mailboxes],
  )

  const headerButton = (
    <button
      type="button"
      className="ph-focus-ring mx-1.5 mt-1 flex h-[30px] w-[calc(100%-0.75rem)] items-center gap-2 rounded-[5px] px-2 text-left transition-colors hover:bg-[var(--sidebar-accent)]"
      onClick={() => setCollapsed((prev) => !prev)}
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
          <ContextMenuItem onSelect={() => setCollapsed((prev) => !prev)}>
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
              depth={1}
              onOpenAccountSettings={onOpenAccountSettings}
              isSelected={
                selectedView?.kind === 'source-mailbox' &&
                selectedView.sourceId === source.id &&
                selectedView.mailboxId === mailbox.id
              }
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

/** Collapsible section header button. */
function SectionHeader({
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

/**
 * Sidebar navigation with smart mailbox and source mailbox sections.
 *
 * @spec docs/L1-ui#component-hierarchy
 * @spec docs/L0-ui#navigation-model
 */
export function Sidebar({
  selectedView,
  onOpenAccountSettings,
  onOpenSmartMailboxSettings,
  onSelectSmartMailbox,
  onSelectSourceMailbox,
  onSelectTag,
  onSyncSource,
}: SidebarProps) {
  const { error, isLoading, refetchBootstrap, smartMailboxes, sources, tags } =
    useMailboxNavigationReadModels()

  const [mailboxesCollapsed, setMailboxesCollapsed] = useState(false)
  const [sourcesCollapsed, setSourcesCollapsed] = useState(false)
  const groupedSmartMailboxes = useMemo(
    () => partitionSmartMailboxes(smartMailboxes),
    [smartMailboxes],
  )

  return (
    <aside className="flex h-full min-h-0 min-w-0 flex-col bg-sidebar text-sidebar-foreground">
      <nav className="ph-scroll min-h-0 flex-1 overflow-y-auto px-2 pb-4 pt-3">
        {isLoading && (
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
        )}
        {error && (
          <div className="px-3 py-4">
            <div className="flex flex-col items-center gap-2 text-center">
              <AlertCircle size={20} className="text-destructive/60" />
              <p className="text-xs text-destructive">Failed to load sidebar</p>
              <button
                type="button"
                className="text-xs text-muted-foreground underline underline-offset-2 hover:text-foreground"
                onClick={() => void refetchBootstrap()}
              >
                Try again
              </button>
            </div>
          </div>
        )}
        {!isLoading && !error && (
          <>
            {groupedSmartMailboxes.quick.length > 0 && (
              <div className="space-y-0.5 pb-3">
                {groupedSmartMailboxes.quick.map((smartMailbox) => (
                  <ViewItem
                    key={smartMailbox.id}
                    id={smartMailbox.id}
                    name={smartMailbox.name}
                    unreadMessages={smartMailbox.unreadMessages}
                    accent={smartMailboxAccent(smartMailbox.name)}
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
            <SectionHeader
              label="Smart"
              collapsed={mailboxesCollapsed}
              onToggle={() => setMailboxesCollapsed((prev) => !prev)}
            />
            {!mailboxesCollapsed && (
              <div className="space-y-0.5 py-1">
                {groupedSmartMailboxes.smart.map((smartMailbox) => (
                  <ViewItem
                    key={smartMailbox.id}
                    id={smartMailbox.id}
                    name={smartMailbox.name}
                    unreadMessages={smartMailbox.unreadMessages}
                    accent={smartMailboxAccent(smartMailbox.name)}
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

            {tags.length > 0 && (
              <>
                <SectionHeader
                  label="Tags"
                  collapsed={false}
                  onToggle={() => {}}
                />
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
            )}

            <SectionHeader
              label="Accounts"
              collapsed={sourcesCollapsed}
              onToggle={() => setSourcesCollapsed((prev) => !prev)}
            />
            {!sourcesCollapsed && (
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
        )}
      </nav>
    </aside>
  )
}
