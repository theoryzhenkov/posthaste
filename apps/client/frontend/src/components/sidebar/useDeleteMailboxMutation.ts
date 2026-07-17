import { useMutation } from '@tanstack/react-query'

import { useCommands } from '@/data/commands'

/**
 * Delete a mailbox on a source.
 *
 * Not optimistic (mirroring the create): the `deleteMailbox` command awaits
 * the provider destroy, and its acceptance invalidates every query, so the
 * mailbox disappears from the sidebar with the refreshed `mailboxCounts`
 * answer.
 *
 * SAFETY: `removeEmails` is the confirm-with-count flag. A non-empty mailbox
 * delete without it is refused with a `conflict` error; the caller (the
 * delete dialog) detects that to re-prompt with the fresh count. Generic
 * errors are surfaced by the caller via a toast — the raw provider string is
 * never leaked.
 */
export function useDeleteMailboxMutation(accountId: string) {
  const commands = useCommands()
  return useMutation({
    mutationFn: ({
      mailboxId,
      removeEmails,
    }: {
      mailboxId: string
      removeEmails: boolean
    }) => commands.deleteMailbox(accountId, mailboxId, removeEmails),
  })
}
