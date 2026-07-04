/**
 * Cache-only lookup from a message's `(sourceId, mailboxIds)` to the mailbox
 * name/role it belongs to, for the "show source mailbox" row chip.
 *
 * Reads the same per-account `queryKeys.mailboxes` read model the sidebar
 * uses (seeded by the navigation bootstrap) via `enabled: false` observers —
 * no query of its own, and critically no per-row network fetch: this hook
 * subscribes once per account, not once per message.
 *
 * @spec docs/L1-ui#messagelist
 */
import { useMemo } from 'react'
import { useQueries } from '@tanstack/react-query'

import type { AccountDirectory } from '@/accountDirectory'
import type { Mailbox, MessageSummary } from '@/api/types'
import { queryKeys } from '@/queryKeys'
import { runtimeViews } from '@/runtime/views'

export interface ResolvedMailbox {
  mailbox: Mailbox
  /** True when the message's account directory has more than one enabled
   *  account, so the chip should disambiguate with the account name. */
  isMultiAccount: boolean
  accountName: string
}

export interface MailboxDirectory {
  /**
   * Resolve the mailbox to show in the chip for a message, excluding
   * `excludeMailboxId` (the mailbox already being viewed, if any) when
   * possible. Prefers a role-bearing membership, else the alphabetically
   * first name; returns null when nothing is resolvable (e.g. the account's
   * mailboxes haven't loaded yet) so the caller can render no chip.
   */
  resolve: (
    message: MessageSummary,
    excludeMailboxId: string | null,
  ) => ResolvedMailbox | null
}

export function useMailboxDirectory(
  accountDirectory: AccountDirectory,
): MailboxDirectory {
  const accounts = accountDirectory.accounts
  const mailboxQueries = useQueries({
    queries: accounts.map((account) => ({
      queryKey: queryKeys.mailboxes(account.id),
      queryFn: () => runtimeViews.mail.mailboxes(account.id),
      enabled: false,
    })),
  })

  return useMemo(() => {
    const bySource = new Map<string, Map<string, Mailbox>>()
    accounts.forEach((account, index) => {
      const mailboxes = mailboxQueries[index]?.data ?? []
      bySource.set(account.id, new Map(mailboxes.map((m) => [m.id, m])))
    })
    const isMultiAccount = accounts.length > 1

    return {
      resolve: (message, excludeMailboxId) => {
        const mailboxesById = bySource.get(message.sourceId)
        if (!mailboxesById || mailboxesById.size === 0) return null

        const candidateIds = message.mailboxIds.filter(
          (id) => id !== excludeMailboxId,
        )
        const idsToTry =
          candidateIds.length > 0 ? candidateIds : message.mailboxIds

        const resolved = idsToTry
          .map((id) => mailboxesById.get(id))
          .filter((mailbox): mailbox is Mailbox => mailbox !== undefined)
        if (resolved.length === 0) return null

        const chosen =
          resolved.find((mailbox) => mailbox.role !== null) ??
          [...resolved].sort((a, b) => a.name.localeCompare(b.name))[0]

        return {
          mailbox: chosen,
          isMultiAccount,
          accountName: accountDirectory.resolveAccountName(
            message.sourceId,
            message.sourceName,
          ),
        }
      },
    }
  }, [accounts, mailboxQueries, accountDirectory])
}
