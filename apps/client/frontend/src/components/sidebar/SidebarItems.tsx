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
import { RenameMailboxDialog } from './RenameMailboxDialog'
import { isMailboxDeletable, isMailboxRenamable, itemButtonClass } from './model'

function roleIcon(role: Mailbox['role'], size = 14): ReactNode {
  return renderMailboxRoleIcon(role, size)
}

/**
 * The shared "Add to group ▸ [groups | New group…]" submenu + "Remove from
 * group" item, reused by both {@link MailboxItem} (source) and
 * {@link SmartMailboxItem} (smart). The caller owns the New-group dialog state
 * (the dialog must live OUTSIDE the context menu so it survives the menu
 * closing), so this renders only the menu rows and delegates "New group…" via
 * `onRequestNewGroup`. `groups` is already filtered by the caller to the groups
 * this entity may join (source: its source's groups; smart: smart-homogeneous
 * groups only), which is what keeps smart and source groups from ever mixing.
 */
function GroupSubmenuItems({
  entityId,
  groups,
  currentGroupId,
  onAssignToGroup,
  onRemoveFromGroup,
  onRequestNewGroup,
}: {
  entityId: string
  groups: readonly MailboxGroup[]
  currentGroupId: string | null
  onAssignToGroup?: (groupId: string, mailboxId: string) => void
  onRemoveFromGroup?: (mailboxId: string) => void
  onRequestNewGroup: () => void
}) {
  return (
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
              onSelect={() => onAssignToGroup?.(group.id, entityId)}
            >
              <span className="flex w-4 justify-center">
                {group.id === currentGroupId ? <Check size={14} /> : null}
              </span>
              <span className="min-w-0 flex-1 truncate">{group.name}</span>
            </ContextMenuItem>
          ))}
          {groups.length > 0 && <ContextMenuSeparator />}
          <ContextMenuItem onSelect={onRequestNewGroup}>
            <Plus size={14} />
            New group…
          </ContextMenuItem>
        </ContextMenuSubContent>
      </ContextMenuSub>
      {currentGroupId != null && (
        <ContextMenuItem onSelect={() => onRemoveFromGroup?.(entityId)}>
          <FolderMinus size={14} />
          Remove from group
        </ContextMenuItem>
      )}
    </>
  )
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
  depth = 0,
  groups = [],
  currentGroupId = null,
  onOpenSettings,
  onSelect,
  onAssignToGroup,
  onRemoveFromGroup,
  onCreateGroup,
}: {
  id: string
  name: string
  role: string | null
  defaultKey: string | null
  unreadMessages?: number
  accent?: string
  isSelected: boolean
  isPaneActive?: boolean
  /** Indent level: 0 for ungrouped, 1 for a member nested under a Group header. */
  depth?: number
  /** Smart-homogeneous Groups this smart mailbox may join (source groups are
   *  filtered out by the caller to keep groups from mixing). */
  groups?: readonly MailboxGroup[]
  /** The group this smart mailbox currently belongs to, if any. */
  currentGroupId?: string | null
  onOpenSettings: (smartMailboxId: string) => void
  onSelect: () => void
  /** Assign this smart mailbox to an existing group (presentational, synced). */
  onAssignToGroup?: (groupId: string, mailboxId: string) => void
  /** Remove this smart mailbox from its current group (back to ungrouped). */
  onRemoveFromGroup?: (mailboxId: string) => void
  /** Create a new group seeded with this smart mailbox. */
  onCreateGroup?: (name: string, seedMailboxId: string) => void
}) {
  const [isNewGroupOpen, setIsNewGroupOpen] = useState(false)
  const groupsEnabled = onAssignToGroup != null && onCreateGroup != null
  const button = (
    <button
      className={itemButtonClass(isSelected, depth, isPaneActive)}
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
    <>
      <ContextMenu>
        <ContextMenuTrigger asChild>{button}</ContextMenuTrigger>
        <ContextMenuContent className="min-w-44">
          <ContextMenuItem onSelect={onSelect}>
            <MailOpen size={14} />
            Open
          </ContextMenuItem>
          {groupsEnabled && (
            <GroupSubmenuItems
              entityId={id}
              groups={groups}
              currentGroupId={currentGroupId}
              onAssignToGroup={onAssignToGroup}
              onRemoveFromGroup={onRemoveFromGroup}
              onRequestNewGroup={() => setIsNewGroupOpen(true)}
            />
          )}
          <ContextMenuSeparator />
          <ContextMenuItem onSelect={() => onOpenSettings(id)}>
            <Edit3 size={14} />
            Edit mailbox
          </ContextMenuItem>
        </ContextMenuContent>
      </ContextMenu>
      {groupsEnabled && (
        <GroupNameDialog
          mode="create"
          open={isNewGroupOpen}
          onOpenChange={setIsNewGroupOpen}
          onSubmit={(name) => onCreateGroup?.(name, id)}
        />
      )}
    </>
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
  // Counts ride the `mailboxCounts` answer itself: the `mailbox` prop is the
  // answered row, kept live by the generation-advance invalidation.
  const unread = mailbox.unreadEmails
  const [isDeleteOpen, setIsDeleteOpen] = useState(false)
  const [isRenameOpen, setIsRenameOpen] = useState(false)
  const isDeletable = isMailboxDeletable(mailbox)
  const isRenamable = isMailboxRenamable(mailbox)
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
            <GroupSubmenuItems
              entityId={mailbox.id}
              groups={groups}
              currentGroupId={currentGroupId}
              onAssignToGroup={onAssignToGroup}
              onRemoveFromGroup={onRemoveFromGroup}
              onRequestNewGroup={() => setIsNewGroupOpen(true)}
            />
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
          {(isRenamable || isDeletable) && <ContextMenuSeparator />}
          {isRenamable && (
            <ContextMenuItem onSelect={() => setIsRenameOpen(true)}>
              <Edit3 size={14} />
              Rename mailbox
            </ContextMenuItem>
          )}
          {isDeletable && (
            <ContextMenuItem
              variant="destructive"
              onSelect={() => setIsDeleteOpen(true)}
            >
              <Trash2 size={14} />
              Delete mailbox
            </ContextMenuItem>
          )}
        </ContextMenuContent>
      </ContextMenu>
      {isRenamable && (
        <RenameMailboxDialog
          sourceId={sourceId}
          mailbox={mailbox}
          open={isRenameOpen}
          onOpenChange={setIsRenameOpen}
        />
      )}
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
