import type { ReactNode } from 'react'
import { Edit3, MailOpen, RefreshCw, Settings } from 'lucide-react'

import type { Mailbox, TagSummary } from '@/api/types'
import { cn } from '@/lib/utils'
import {
  mailboxRoleAccent,
  mailboxRoleFromName,
  renderMailboxRoleIcon,
  smartMailboxAccent,
  smartMailboxFallbackIcon,
} from '@/mailboxRoles'

import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from '../ui/context-menu'
import { itemButtonClass } from './model'

function roleIcon(role: Mailbox['role'], size = 14): ReactNode {
  return renderMailboxRoleIcon(role, size)
}

function smartMailboxIcon(name: string, size = 14): ReactNode {
  return renderMailboxRoleIcon(
    mailboxRoleFromName(name),
    size,
    smartMailboxFallbackIcon(name),
  )
}

export function SmartMailboxItem({
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
        {name}
      </span>
      {unreadMessages != null && unreadMessages > 0 && (
        <UnreadCount count={unreadMessages} isSelected={isSelected} />
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

export function TagItem({
  tag,
  onSelect,
}: {
  tag: TagSummary
  onSelect: () => void
}) {
  return (
    <button className={itemButtonClass(false)} onClick={onSelect} type="button">
      <span
        className="flex w-4 justify-center"
        style={{ color: smartMailboxAccent(tag.name) }}
      >
        {smartMailboxIcon(tag.name)}
      </span>
      <span className="min-w-0 flex-1 truncate">{tag.name}</span>
      {tag.unreadMessages > 0 && <UnreadCount count={tag.unreadMessages} />}
    </button>
  )
}

export function MailboxItem({
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
        <UnreadCount count={mailbox.unreadEmails} isSelected={isSelected} />
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

function UnreadCount({
  count,
  isSelected = false,
}: {
  count: number
  isSelected?: boolean
}) {
  return (
    <span
      className={cn(
        'font-mono text-[11px] font-medium tabular-nums',
        isSelected
          ? 'text-[var(--list-selection-foreground)]'
          : 'text-muted-foreground/80',
      )}
    >
      {count}
    </span>
  )
}
