/**
 * Client-side, cross-device-synced sidebar "Groups" (presentation only).
 *
 * Groups ride the SAME synced-settings path as `mailboxColors` /
 * `smartMailboxOrder`: read reactively from the settings query, and every
 * change is applied optimistically to the settings cache then persisted via
 * `runtimeMutations.settings.patch({ mailboxGroups })` (the single source of
 * truth the backend re-resolves from), then invalidated to reconcile — which
 * also self-corrects on a failed patch. No provider interaction ever occurs.
 *
 * Scope guards enforced here:
 *  - Deleting a group NEVER deletes mailboxes or mail — it only drops the group
 *    entry, so its members fall back to ungrouped.
 *  - A mailbox belongs to AT MOST ONE group — assigning/creating removes it from
 *    any other group first.
 *
 * @spec docs/eph/RFC-L2-mailbox-management#a4
 */
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useCallback, useMemo } from 'react'
import { toast } from 'sonner'

import type { AppSettings, MailboxGroup } from '@/api/types'
import { queryKeys } from '@/queryKeys'
import { runtimeMutations } from '@/runtime/mutations'
import { runtimeViews } from '@/runtime/views'

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
  const { data } = useQuery<AppSettings>({
    queryKey: queryKeys.settings,
    queryFn: runtimeViews.settings.current,
  })
  return data?.mailboxGroups ?? EMPTY_GROUPS
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
  const queryClient = useQueryClient()

  // Apply a pure transform to the current groups, optimistically update the
  // settings cache, persist through the settings patch, then reconcile.
  const apply = useCallback(
    (transform: (groups: readonly MailboxGroup[]) => MailboxGroup[]) => {
      const current =
        queryClient.getQueryData<AppSettings>(queryKeys.settings)
          ?.mailboxGroups ?? []
      const nextGroups = transform(current)
      queryClient.setQueryData<AppSettings>(queryKeys.settings, (prev) =>
        prev ? { ...prev, mailboxGroups: nextGroups } : prev,
      )
      void runtimeMutations.settings
        .patch({ mailboxGroups: nextGroups })
        .catch(() => {
          toast.error("Couldn't update groups. Please try again.")
        })
        .finally(() => {
          void queryClient.invalidateQueries({ queryKey: queryKeys.settings })
        })
      return nextGroups
    },
    [queryClient],
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
