import { useMemo } from 'react'
import { useMailboxCounts, useSmartMailboxes } from '@/data'

/**
 * Resolve a mailbox's role from the `mailboxCounts` family — the domain
 * authority for mailbox metadata, rather than the sidebar's UI-shaped
 * aggregate.
 *
 * Returns null until the data loads, when the ids are absent (e.g. a smart
 * mailbox or search view), or for an unknown/custom role. The underlying
 * query is shared with every other counts consumer and kept fresh by the
 * global generation-advance invalidation, so this costs at most one fetch
 * per account.
 */
export function useMailboxRole(
  sourceId: string | null,
  mailboxId: string | null,
): string | null {
  const { data } = useMailboxCounts(sourceId ?? undefined, {
    enabled: sourceId !== null,
  })
  const rows = data?.rows
  return useMemo(
    () =>
      rows?.find(
        (row) => row.accountId === sourceId && row.mailbox.id === mailboxId,
      )?.mailbox.role ?? null,
    [rows, sourceId, mailboxId],
  )
}

/**
 * Resolve a smart mailbox's assigned role from the `smartMailboxes` family.
 * Returns null until loaded, when the id is absent, or for an unassigned
 * (role-less) smart mailbox. The contextual-action layer uses this to surface
 * role-driven actions (e.g. Delete Permanently when the view's smart mailbox
 * carries the `trash` role).
 */
export function useSmartMailboxRole(
  smartMailboxId: string | null,
): string | null {
  const { data } = useSmartMailboxes({ enabled: smartMailboxId !== null })
  const rows = data?.rows
  return useMemo(
    () => rows?.find((row) => row.id === smartMailboxId)?.role ?? null,
    [rows, smartMailboxId],
  )
}
