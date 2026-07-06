import { useState, type ReactNode } from 'react'
import {
  Check,
  Edit3,
  FolderPlus,
  FolderMinus,
  MailOpen,
  Plus,
  RefreshCw,
  Settings,
  Trash2,
} from 'lucide-react'

import type { Mailbox, MailboxGroup } from '@/api/types'
import { accentColor } from '@/design'
import { useMailboxCounts } from '@/live-store/store'
import { cn } from '@/lib/utils'
import {
  mailboxRoleAccent,
  renderMailboxRoleIcon,
  renderSmartMailboxIcon,
} from '@/mailboxRoles'

import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuSub,
  ContextMenuSubContent,
  ContextMenuSubTrigger,
  ContextMenuTrigger,
} from '../ui/context-menu'
import { DeleteMailboxDialog } from './DeleteMailboxDialog'
import { GroupNameDialog } from './GroupNameDialog'
import { isMailboxDeletable, itemButtonClass } from './model'

function roleIcon(role: Mailbox['role'], size = 14): ReactNode {
  return renderMailboxRoleIcon(role, size)
}

export function SmartMailboxItem({
  id,
  name,
  role,
  defaultKey,
  unreadMessages,
  accent,
  isSelected,
  isPaneActive = false,
  onOpenSettings,
  onSelect,
}: {
  id: string
  name: string
  role: string | null
  defaultKey: string | null
  unreadMessages?: number
  accent?: string
  isSelected: boolean
  isPaneActive?: boolean
  onOpenSettings: (smartMailboxId: string) => void
  onSelect: () => void
}) {
  const button = (
    <button
      className={itemButtonClass(isSelected, 0, isPaneActive)}
      onClick={onSelect}
      onContextMenu={onSelect}
      type="button"
    >
      <span
        className="flex w-4 justify-center"
        style={accent ? { color: accent } : undefined}
      >
        {renderSmartMailboxIcon(role, defaultKey)}
      </span>
      <span className="min-w-0 flex-1 truncate">{name}</span>
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

export function MailboxItem({
  sourceId,
  sourceName,
  mailbox,
  colorHue,
  isSelected,
  isPaneActive = false,
  depth = 0,
  groups = [],
  currentGroupId = null,
  onOpenAccountSettings,
  onSelect,
  onSyncSource,
  onAssignToGroup,
  onRemoveFromGroup,
  onCreateGroup,
}: {
  sourceId: string
  sourceName: string
  mailbox: Mailbox
  /** Per-mailbox color override (hue); falls back to the role accent. */
  colorHue?: number
  isSelected: boolean
  isPaneActive?: boolean
  depth?: number
  /** Sidebar Groups available on this source (for the "Add to group" submenu). */
  groups?: readonly MailboxGroup[]
  /** The group this mailbox currently belongs to, if any. */
  currentGroupId?: string | null
  onOpenAccountSettings: (sourceId: string) => void
  onSelect: () => void
  onSyncSource: (sourceId: string) => void
  /** Assign this mailbox to an existing group (presentational, synced). */
  onAssignToGroup?: (groupId: string, mailboxId: string) => void
  /** Remove this mailbox from its current group (back to ungrouped). */
  onRemoveFromGroup?: (mailboxId: string) => void
  /** Create a new group seeded with this mailbox. */
  onCreateGroup?: (name: string, seedMailboxId: string) => void
}) {
  const [isNewGroupOpen, setIsNewGroupOpen] = useState(false)
  const groupsEnabled = onAssignToGroup != null && onCreateGroup != null
  const iconColor =
    colorHue != null ? accentColor(colorHue) : mailboxRoleAccent(mailbox.role)
  // D116: STRUCTURE (name/role/hierarchy) stays request/response from the
  // mailboxes query (the `mailbox` prop); live COUNTS come from the store slice.
  // Fall back to the query's server count when no frame has seeded a live entry
  // yet (bootstrap): a fresh session shows the server count before the first
  // frame arrives, then the live mirror takes over.
  const liveCounts = useMailboxCounts(sourceId)[mailbox.id]
  const unread = liveCounts ? liveCounts.unread : mailbox.unreadEmails
  const [isDeleteOpen, setIsDeleteOpen] = useState(false)
  const isDeletable = isMailboxDeletable(mailbox)
  const button = (
    <button
      className={itemButtonClass(isSelected, depth, isPaneActive)}
      onClick={onSelect}
      onContextMenu={onSelect}
      type="button"
    >
      <span className="flex w-4 justify-center" style={{ color: iconColor }}>
        {roleIcon(mailbox.role)}
      </span>
      <span className="min-w-0 flex-1 truncate">{mailbox.name}</span>
      {unread > 0 && <UnreadCount count={unread} isSelected={isSelected} />}
    </button>
  )

  return (
    <>
      <ContextMenu>
        <ContextMenuTrigger asChild>{button}</ContextMenuTrigger>
        <ContextMenuContent className="min-w-48">
          <ContextMenuItem onSelect={onSelect}>
            <MailOpen size={14} />
            Open mailbox
          </ContextMenuItem>
          {groupsEnabled && (
            <>
              <ContextMenuSeparator />
              <ContextMenuSub>
                <ContextMenuSubTrigger>
                  <FolderPlus size={14} />
                  Add to group
                </ContextMenuSubTrigger>
                <ContextMenuSubContent className="min-w-44">
                  {groups.map((group) => (
                    <ContextMenuItem
                      key={group.id}
                      onSelect={() => onAssignToGroup?.(group.id, mailbox.id)}
                    >
                      <span className="flex w-4 justify-center">
                        {group.id === currentGroupId ? (
                          <Check size={14} />
                        ) : null}
                      </span>
                      <span className="min-w-0 flex-1 truncate">
                        {group.name}
                      </span>
                    </ContextMenuItem>
                  ))}
                  {groups.length > 0 && <ContextMenuSeparator />}
                  <ContextMenuItem onSelect={() => setIsNewGroupOpen(true)}>
                    <Plus size={14} />
                    New group…
                  </ContextMenuItem>
                </ContextMenuSubContent>
              </ContextMenuSub>
              {currentGroupId != null && (
                <ContextMenuItem
                  onSelect={() => onRemoveFromGroup?.(mailbox.id)}
                >
                  <FolderMinus size={14} />
                  Remove from group
                </ContextMenuItem>
              )}
            </>
          )}
          <ContextMenuSeparator />
          <ContextMenuItem onSelect={() => onSyncSource(sourceId)}>
            <RefreshCw size={14} />
            Sync {sourceName}
          </ContextMenuItem>
          <ContextMenuItem onSelect={() => onOpenAccountSettings(sourceId)}>
            <Settings size={14} />
            Account settings
          </ContextMenuItem>
          {isDeletable && (
            <>
              <ContextMenuSeparator />
              <ContextMenuItem
                variant="destructive"
                onSelect={() => setIsDeleteOpen(true)}
              >
                <Trash2 size={14} />
                Delete mailbox
              </ContextMenuItem>
            </>
          )}
        </ContextMenuContent>
      </ContextMenu>
      {isDeletable && (
        <DeleteMailboxDialog
          sourceId={sourceId}
          mailbox={mailbox}
          open={isDeleteOpen}
          onOpenChange={setIsDeleteOpen}
        />
      )}
      {groupsEnabled && (
        <GroupNameDialog
          mode="create"
          open={isNewGroupOpen}
          onOpenChange={setIsNewGroupOpen}
          onSubmit={(name) => onCreateGroup?.(name, mailbox.id)}
        />
      )}
    </>
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
