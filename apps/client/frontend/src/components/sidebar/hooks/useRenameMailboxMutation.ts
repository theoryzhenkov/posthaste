import { useMutation } from '@tanstack/react-query'
import { toast } from 'sonner'

import { MailApiError } from '@/data/transport/client'
import { useCommands } from '@/data/transport/commands'

/**
 * Rename a mailbox on a source.
 *
 * Not optimistic (mirroring create/delete): the `renameMailbox` command
 * awaits the provider update, and its acceptance invalidates every query, so
 * the new name surfaces in the sidebar from the refreshed `mailboxCounts`
 * answer under the same mailbox id. On failure we show a toast — a
 * non-retryable `unavailable` is the transport refusing renames outright
 * (IMAP accounts), everything else gets the generic try-again message; the
 * raw provider string is never leaked.
 */
export function useRenameMailboxMutation(accountId: string) {
  const commands = useCommands()
  return useMutation({
    mutationFn: ({ mailboxId, name }: { mailboxId: string; name: string }) =>
      commands.renameMailbox(accountId, mailboxId, name),
    onError: (error) => {
      if (
        error instanceof MailApiError &&
        error.kind === 'unavailable' &&
        !error.retryable
      ) {
        toast.error("This account doesn't support renaming mailboxes.")
        return
      }
      toast.error("Couldn't rename the mailbox. Please try again.")
    },
  })
}
