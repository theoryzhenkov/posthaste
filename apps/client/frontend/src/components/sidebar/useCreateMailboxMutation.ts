import { useMutation } from '@tanstack/react-query'
import { toast } from 'sonner'

import { useCommands } from '@/data/commands'

/**
 * Create a new top-level mailbox on a source.
 *
 * Not optimistic (mirroring the delete): the `createMailbox` command awaits
 * the provider create, and its acceptance invalidates every query, so the
 * new mailbox surfaces in the sidebar from the refreshed `mailboxCounts`
 * answer. On failure we show a generic toast — the raw provider string is
 * never leaked to the user.
 */
export function useCreateMailboxMutation(accountId: string) {
  const commands = useCommands()
  return useMutation({
    mutationFn: (name: string) => commands.createMailbox(accountId, name),
    onError: () => {
      toast.error("Couldn't create the mailbox. Please try again.")
    },
  })
}
