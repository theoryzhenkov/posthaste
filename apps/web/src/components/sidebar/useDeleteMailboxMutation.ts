import { useMutation, useQueryClient } from '@tanstack/react-query'

import type { Mailbox } from '@/api/types'
import { invalidateAccountReadModels } from '@/domainCache'
import { queryKeys } from '@/queryKeys'
import { runtimeMutations } from '@/runtime/mutations'

/**
 * Delete a mailbox on a source.
 *
 * Mailbox mutations are synchronous, not optimistic (mirroring
 * `useCreateMailboxMutation`): the backend awaits the provider gateway destroy +
 * a resync, then returns the refreshed mailbox list, which seeds the cache so the
 * mailbox disappears from the sidebar.
 *
 * SAFETY: `removeEmails` is the confirm-with-count flag. A non-empty mailbox
 * delete without it is refused by the backend with a 409 `mailbox_not_empty`
 * ({@link ApiError}); the caller (the delete dialog) detects that to re-prompt
 * with the fresh count. Generic errors are surfaced by the caller via a toast —
 * the raw provider/runtime string is never leaked.
 *
 * @spec docs/eph/RFC-L2-mailbox-management
 */
export function useDeleteMailboxMutation(accountId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({
      mailboxId,
      removeEmails,
    }: {
      mailboxId: string
      removeEmails: boolean
    }) =>
      runtimeMutations.mailboxes.delete(accountId, mailboxId, { removeEmails }),
    onSuccess: (nextMailboxes: Mailbox[]) => {
      queryClient.setQueryData(queryKeys.mailboxes(accountId), nextMailboxes)
      invalidateAccountReadModels(queryClient, accountId)
    },
  })
}
