import { useMutation } from '@tanstack/react-query'

import type { KnownMailboxRole } from '@/data/transport/api'
import { useCommands } from '@/data'

/**
 * Mailbox role-switch mutation over the `setMailboxRole` command. The
 * backend clears the role from any other mailbox that held it; acceptance
 * rides the global invalidation cycle, so the mailbox rows re-serve with the
 * new role assignment. The provider reconciliation (slow on some hosts)
 * continues server-side after acceptance.
 */
export function useMailboxRoleMutation(accountId: string, mailboxId: string) {
  const commands = useCommands()
  return useMutation({
    mutationFn: (role: KnownMailboxRole | null) =>
      commands.setMailboxRole(accountId, mailboxId, role),
  })
}
