/**
 * Client-side, cross-device-synced sidebar "Groups" (presentation only).
 *
 * Groups ride the SAME synced-settings path as `mailboxColors` /
 * `smartMailboxOrder`: read reactively from the `appSettings` answer, and
 * every change is a read-modify-write of the settings document through the
 * `updateSettings` command — acceptance invalidates every query, so the
 * sidebar re-renders from the backend's re-resolved answer. No provider
 * interaction ever occurs.
 *
 * Scope guards enforced here:
 *  - Deleting a group NEVER deletes mailboxes or mail — it only drops the group
 *    entry, so its members fall back to ungrouped.
 *  - A mailbox belongs to AT MOST ONE group — assigning/creating removes it from
 *    any other group first.
 */
import { useQueryClient } from '@tanstack/react-query'
import { useCallback, useMemo } from 'react'
import { toast } from 'sonner'

import { useMailClient } from '@/data/context'
import { runCommand } from '@/data/commands'
import { ensureAppSettings, useAppSettings } from '@/data/queries'
import type { MailboxGroup } from '@/gen'

const EMPTY_GROUPS: readonly MailboxGroup[] = Object.freeze([])

function newGroupId(): string {
  if (
    typeof crypto !== 'undefined' &&
    typeof crypto.randomUUID === 'function'
  ) {
    return `group-${crypto.randomUUID()}`
  }
  return `group-${Date.now()}-${Math.random().toString(36).slice(2)}`
}

/** Reactive read of the synced sidebar Groups. */
export function useMailboxGroups(): readonly MailboxGroup[] {
  const { data } = useAppSettings()
  return data?.settings.mailboxGroups ?? EMPTY_GROUPS
}

/** Drop a mailbox from every group (pure helper; keeps one-group-per-mailbox). */
function withoutMember(
  groups: readonly MailboxGroup[],
  mailboxId: string,
): MailboxGroup[] {
  return groups.map((group) =>
    group.mailboxIds.includes(mailboxId)
      ? {
          ...group,
          mailboxIds: group.mailboxIds.filter((id) => id !== mailboxId),
        }
      : group,
  )
}

/** Drop groups that no longer hold any mailbox — an empty group renders nowhere
 *  (partitioning surfaces a group only under the source of its members), so we
 *  prune it rather than leave an invisible orphan. Never removes mail. */
function pruneEmpty(groups: readonly MailboxGroup[]): MailboxGroup[] {
  return groups.filter((group) => group.mailboxIds.length > 0)
}

export interface MailboxGroupMutations {
  /** Create a group; optionally seed it with one mailbox (removed from any other
   *  group first). Returns the new group's id. */
  createGroup: (name: string, seedMailboxId?: string) => string
  renameGroup: (groupId: string, name: string) => void
  /** Delete a group — its members become ungrouped. Never touches mailboxes. */
  deleteGroup: (groupId: string) => void
  /** Assign a mailbox to a group (removing it from any other group). */
  assignToGroup: (groupId: string, mailboxId: string) => void
  /** Remove a mailbox from whatever group holds it (back to ungrouped). */
  removeFromGroup: (mailboxId: string) => void
}

export function useMailboxGroupMutations(): MailboxGroupMutations {
  const client = useMailClient()
  const queryClient = useQueryClient()

  // Apply a pure transform to the current groups: read the settings
  // document, transform its groups, and write the document back whole.
  const apply = useCallback(
    (transform: (groups: readonly MailboxGroup[]) => MailboxGroup[]) => {
      void (async () => {
        const settings = await ensureAppSettings(client, queryClient)
        await runCommand(client, queryClient, {
          updateSettings: {
            settings: {
              ...settings,
              mailboxGroups: transform(settings.mailboxGroups),
            },
            forceBackfill: false,
          },
        })
      })().catch(() => {
        toast.error("Couldn't update groups. Please try again.")
      })
    },
    [client, queryClient],
  )

  return useMemo<MailboxGroupMutations>(() => {
    return {
      createGroup: (name, seedMailboxId) => {
        const id = newGroupId()
        apply((groups) => {
          const nextOrder =
            groups.reduce((max, group) => Math.max(max, group.order), -1) + 1
          const base = seedMailboxId
            ? withoutMember(groups, seedMailboxId)
            : [...groups]
          return [
            ...base,
            {
              id,
              name,
              mailboxIds: seedMailboxId ? [seedMailboxId] : [],
              order: nextOrder,
            },
          ]
        })
        return id
      },
      renameGroup: (groupId, name) => {
        apply((groups) =>
          groups.map((group) =>
            group.id === groupId ? { ...group, name } : group,
          ),
        )
      },
      deleteGroup: (groupId) => {
        // Delete-never-deletes-mail: just drop the group entry; members fall
        // back to ungrouped. No provider/mailbox mutation.
        apply((groups) => groups.filter((group) => group.id !== groupId))
      },
      assignToGroup: (groupId, mailboxId) => {
        apply((groups) =>
          pruneEmpty(
            withoutMember(groups, mailboxId).map((group) =>
              group.id === groupId
                ? { ...group, mailboxIds: [...group.mailboxIds, mailboxId] }
                : group,
            ),
          ),
        )
      },
      removeFromGroup: (mailboxId) => {
        apply((groups) => pruneEmpty(withoutMember(groups, mailboxId)))
      },
    }
  }, [apply])
}
