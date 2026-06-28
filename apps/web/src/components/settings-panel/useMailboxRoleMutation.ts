import { useMutation, useQueryClient } from '@tanstack/react-query'

import type { KnownMailboxRole, Mailbox } from '@/api/types'
import { invalidateAccountReadModels } from '@/domainCache'
import { queryKeys } from '@/queryKeys'
import { runtimeMutations } from '@/runtime/mutations'

/**
 * Mailbox role-switch mutation with a React Query optimistic update.
 *
 * The backend `set_mailbox_role` awaits the provider gateway + a manual full
 * sync (slow, especially on Gmail); without optimism the role Select blocks on
 * that round-trip. This applies the role change to the mailboxes cache
 * immediately + reconciles on success / rolls back on error. The backend clears
 * the role from any other mailbox that held it — mirror that optimistically so
 * two mailboxes don't briefly show the same role.
 *
 * @spec docs/L1-api#mailbox-metadata
 */
export function useMailboxRoleMutation(accountId: string, mailboxId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (role: KnownMailboxRole | null) =>
      runtimeMutations.mailboxes.patch(accountId, mailboxId, { role }),
    onMutate: async (role) => {
      const key = queryKeys.mailboxes(accountId)
      await queryClient.cancelQueries({ queryKey: key })
      const previous = queryClient.getQueryData<Mailbox[]>(key)
      queryClient.setQueryData<Mailbox[]>(key, (old) =>
        old?.map((m) => {
          if (m.id === mailboxId) {
            return { ...m, role: role ?? null }
          }
          if (role && m.role === role) {
            return { ...m, role: null }
          }
          return m
        }),
      )
      return { previous }
    },
    onError: (_error, _role, context) => {
      if (context?.previous) {
        queryClient.setQueryData(
          queryKeys.mailboxes(accountId),
          context.previous,
        )
      }
    },
    onSuccess: (nextMailboxes) => {
      queryClient.setQueryData(queryKeys.mailboxes(accountId), nextMailboxes)
      invalidateAccountReadModels(queryClient, accountId)
    },
  })
}
