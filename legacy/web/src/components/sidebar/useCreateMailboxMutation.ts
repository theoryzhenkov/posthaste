import { useMutation, useQueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'

import type { Mailbox } from '@/api/types'
import { invalidateAccountReadModels } from '@/domainCache'
import { queryKeys } from '@/queryKeys'
import { runtimeMutations } from '@/runtime/mutations'

/**
 * Create a new top-level mailbox on a source.
 *
 * Mailbox mutations are synchronous, not optimistic (mirroring
 * `useMailboxRoleMutation`): the backend awaits the provider gateway create + a
 * resync, then returns the refreshed mailbox list, which seeds the cache so the
 * new mailbox surfaces in the sidebar. On failure we show a generic toast — the
 * raw provider/runtime string is never leaked to the user.
 *
 * @spec docs/eph/RFC-L2-mailbox-management
 */
export function useCreateMailboxMutation(accountId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (name: string) =>
      runtimeMutations.mailboxes.create(accountId, { name }),
    onSuccess: (nextMailboxes: Mailbox[]) => {
      queryClient.setQueryData(queryKeys.mailboxes(accountId), nextMailboxes)
      invalidateAccountReadModels(queryClient, accountId)
    },
    onError: () => {
      toast.error("Couldn't create the mailbox. Please try again.")
    },
  })
}
