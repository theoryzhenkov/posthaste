import { useQuery } from '@tanstack/react-query'
import { useMemo } from 'react'
import { queryKeys } from '../queryKeys'
import { fetchRuntimeMailboxes } from '../runtime/adapter'

/**
 * Resolve a mailbox's role from the per-account mailbox read model
 * (`queryKeys.mailboxes`) — the domain authority for mailbox metadata, rather
 * than the sidebar's UI-shaped aggregate.
 *
 * Returns null until the data loads, when the ids are absent (e.g. a smart
 * mailbox or search view), or for an unknown/custom role. The underlying query
 * is React Query-cached, shared with the settings editors, and kept fresh by
 * `domainCache` invalidation, so this costs at most one fetch per account.
 */
export function useMailboxRole(
  sourceId: string | null,
  mailboxId: string | null,
): string | null {
  const { data: mailboxes } = useQuery({
    queryKey: queryKeys.mailboxes(sourceId),
    queryFn: () => fetchRuntimeMailboxes(sourceId!),
    enabled: sourceId !== null,
  })
  return useMemo(
    () => mailboxes?.find((mailbox) => mailbox.id === mailboxId)?.role ?? null,
    [mailboxes, mailboxId],
  )
}
