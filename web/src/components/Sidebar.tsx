/**
 * Left-pane sidebar with smart mailbox and source mailbox navigation.
 *
 * Loads data from `GET /v1/sidebar` and renders collapsible sections
 * for smart mailboxes and per-source mailbox trees.
 *
 * @spec docs/L1-ui#component-hierarchy
 * @spec docs/L0-ui#navigation-model
 */
import { useQuery } from '@tanstack/react-query'
import { useMemo, useState } from 'react'
import { AlertCircle, ChevronDown, ChevronRight } from 'lucide-react'
import { fetchSidebar } from '../api/client'
import type {
  Mailbox,
  SidebarResponse,
  SidebarSmartMailbox,
} from '../api/types'
import { cn } from '../lib/utils'
import {
  mailboxRoleFromName,
  renderMailboxRoleIcon,
  smartMailboxFallbackIcon,
} from '../mailboxRoles'

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
  onSelectSmartMailbox: (smartMailboxId: string, name: string) => void
  onSelectSourceMailbox: (
    sourceId: string,
    mailboxId: string,
    name: string,
  ) => void
}

function roleIcon(role: Mailbox['role'], size = 14): React.ReactNode {
  return renderMailboxRoleIcon(role, size)
}

function mailboxRoleAccent(role: Mailbox['role']): string {
  switch (role) {
    case 'inbox':
      return '#2B7EC2'
    case 'archive':
      return '#3D8B6D'
    case 'drafts':
      return '#8B5CF6'
    case 'sent':
      return '#D96A42'
    case 'junk':
      return '#C5A100'
    case 'trash':
      return '#8A5B4B'
    default:
      return '#7E8691'
  }
}

const SOURCE_SWATCHES = [
  '#2B7EC2',
  '#D96A42',
  '#3D8B6D',
  '#8B5CF6',
  '#C5A100',
] as const

const SIDEBAR_ACCENT = {
  blue: 'oklch(0.65 0.13 245)',
  coral: 'oklch(0.68 0.17 45)',
  sage: 'oklch(0.68 0.08 145)',
  amber: 'oklch(0.78 0.13 78)',
  violet: 'oklch(0.65 0.13 295)',
  rose: 'oklch(0.70 0.15 12)',
  muted: 'oklch(0.60 0.008 70)',
} as const

function sourceStamp(sourceName: string): string {
  return sourceName.trim().charAt(0).toUpperCase() || '?'
}

function sourceAccent(sourceId: string, sourceName: string): string {
  const seed = `${sourceId}:${sourceName}`
  let hash = 0
  for (let index = 0; index < seed.length; index += 1) {
    hash = (hash * 31 + seed.charCodeAt(index)) >>> 0
  }
  return SOURCE_SWATCHES[hash % SOURCE_SWATCHES.length]
}

/** Icon for smart mailboxes based on the name heuristic. */
function smartMailboxIcon(name: string, size = 14): React.ReactNode {
  return renderMailboxRoleIcon(
    mailboxRoleFromName(name),
    size,
    smartMailboxFallbackIcon(name),
  )
}

function smartMailboxAccent(name: string): string | undefined {
  const normalized = name.trim().toLowerCase()
  switch (normalized) {
    case 'inbox':
    case 'all inboxes':
    case 'all mail':
    case 'today':
      return SIDEBAR_ACCENT.blue
    case 'flagged':
    case 'relevant':
    case 'sent':
    case 'follow-up':
      return SIDEBAR_ACCENT.coral
    case 'read later':
    case 'read-later':
    case 'junk':
    case 'spam':
      return SIDEBAR_ACCENT.amber
    case 'bills':
    case 'billing':
    case 'drafts':
      return SIDEBAR_ACCENT.violet
    case 'newsletters':
    case 'personal':
      return SIDEBAR_ACCENT.sage
    case 'trash':
      return SIDEBAR_ACCENT.rose
    case 'archive':
      return SIDEBAR_ACCENT.blue
    case 'work':
      return SIDEBAR_ACCENT.blue
    default:
      return SIDEBAR_ACCENT.muted
  }
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

function partitionSmartMailboxes(smartMailboxes: SidebarSmartMailbox[]) {
  const quick: SidebarSmartMailbox[] = []
  const smart: SidebarSmartMailbox[] = []
  const tags: SidebarSmartMailbox[] = []

  for (const mailbox of smartMailboxes) {
    const priority = smartMailboxPriority(mailbox.name)
    if (priority !== 99) {
      quick.push(mailbox)
      continue
    }

    const role = mailboxRoleFromName(mailbox.name)
    if (role === null) {
      tags.push(mailbox)
      continue
    }

    smart.push(mailbox)
  }

  quick.sort(
    (left, right) =>
      smartMailboxPriority(left.name) - smartMailboxPriority(right.name),
  )
  smart.sort((left, right) => left.name.localeCompare(right.name))
  tags.sort((left, right) => left.name.localeCompare(right.name))

  return { quick, smart, tags }
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
  name,
  unreadMessages,
  accent,
  isSelected,
  onSelect,
}: {
  name: string
  unreadMessages?: number
  accent?: string
  isSelected: boolean
  onSelect: () => void
}) {
  return (
    <button
      className={itemButtonClass(isSelected)}
      onClick={onSelect}
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
}

/** Source mailbox row with role icon and unread badge. */
function MailboxItem({
  mailbox,
  isSelected,
  depth = 0,
  onSelect,
}: {
  mailbox: Mailbox
  isSelected: boolean
  depth?: number
  onSelect: () => void
}) {
  return (
    <button
      className={itemButtonClass(isSelected, depth)}
      onClick={onSelect}
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
}

/** Collapsible source section with its mailbox children. */
function SourceSection({
  source,
  selectedView,
  onSelectSourceMailbox,
}: {
  source: SidebarResponse['sources'][number]
  selectedView: SidebarSelection | null
  onSelectSourceMailbox: (
    sourceId: string,
    mailboxId: string,
    name: string,
  ) => void
}) {
  const [collapsed, setCollapsed] = useState(false)
  const accent = useMemo(
    () => sourceAccent(source.id, source.name),
    [source.id, source.name],
  )
  const unreadTotal = useMemo(
    () =>
      source.mailboxes.reduce((sum, mailbox) => sum + mailbox.unreadEmails, 0),
    [source.mailboxes],
  )

  return (
    <div>
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
        <span
          className="flex size-[18px] shrink-0 items-center justify-center rounded-[4px] font-mono text-[10px] font-bold text-white"
          style={{ backgroundColor: accent }}
        >
          {sourceStamp(source.name)}
        </span>
        <span className="min-w-0 flex-1 truncate text-[13px] font-semibold text-sidebar-foreground">
          {source.name}
        </span>
        {unreadTotal > 0 && (
          <span className="rounded-[4px] bg-signal-unread px-1.5 font-mono text-[11px] font-semibold tabular-nums text-white">
            {unreadTotal}
          </span>
        )}
      </button>
      {!collapsed && (
        <div className="space-y-0.5">
          {source.mailboxes.map((mailbox) => (
            <MailboxItem
              key={`${source.id}:${mailbox.id}`}
              mailbox={mailbox}
              depth={1}
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
  onSelectSmartMailbox,
  onSelectSourceMailbox,
}: SidebarProps) {
  const {
    data: sidebar,
    isLoading,
    error,
    refetch,
  } = useQuery({
    queryKey: ['sidebar'],
    queryFn: fetchSidebar,
  })

  const [mailboxesCollapsed, setMailboxesCollapsed] = useState(false)
  const [sourcesCollapsed, setSourcesCollapsed] = useState(false)
  const groupedSmartMailboxes = useMemo(
    () => partitionSmartMailboxes(sidebar?.smartMailboxes ?? []),
    [sidebar?.smartMailboxes],
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
                onClick={() => void refetch()}
              >
                Try again
              </button>
            </div>
          </div>
        )}
        {sidebar && (
          <>
            {groupedSmartMailboxes.quick.length > 0 && (
              <div className="space-y-0.5 pb-3">
                {groupedSmartMailboxes.quick.map((smartMailbox) => (
                  <ViewItem
                    key={smartMailbox.id}
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
                  />
                ))}
              </div>
            )}

            {groupedSmartMailboxes.tags.length > 0 && (
              <>
                <SectionHeader
                  label="Tags"
                  collapsed={false}
                  onToggle={() => {}}
                />
                <div className="space-y-0.5 py-1">
                  {groupedSmartMailboxes.tags.map((smartMailbox) => (
                    <ViewItem
                      key={smartMailbox.id}
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
                {sidebar.sources.map((source) => (
                  <SourceSection
                    key={source.id}
                    source={source}
                    selectedView={selectedView}
                    onSelectSourceMailbox={onSelectSourceMailbox}
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
