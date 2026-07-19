/**
 * Lookup from a message's `(sourceId, mailboxIds)` to the mailbox name/role
 * it belongs to, for the "show source mailbox" row chip.
 *
 * Reads the same `mailboxCounts` answer the sidebar renders — one query for
 * every account's mailboxes, deduped by the shared family key; critically no
 * per-row fetch.
 *
 */
import { useMemo } from 'react'

import type { AccountDirectory } from '@/data/models/accountDirectory'
import type { Mailbox, MessageSummary } from '@/data/transport/api'
import { useMailboxCounts } from '@/data/queries/queries'

interface ResolvedMailbox {
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
  /**
   * All mailboxes of one account, in answer order — the row-level binding
   * for `ActionServices.mailboxes` (the parameterized "Move to…" options).
   * Empty until the account's mailboxes load, which hides the action rather
   * than offering a bogus empty picker.
   */
  list: (sourceId: string) => Mailbox[]
}

export function useMailboxDirectory(
  accountDirectory: AccountDirectory,
): MailboxDirectory {
  const accounts = accountDirectory.accounts
  const mailboxCounts = useMailboxCounts()
  const rows = mailboxCounts.data?.rows

  return useMemo(() => {
    const bySource = new Map<string, Map<string, Mailbox>>()
    for (const row of rows ?? []) {
      let byId = bySource.get(row.accountId)
      if (!byId) {
        byId = new Map()
        bySource.set(row.accountId, byId)
      }
      byId.set(row.mailbox.id, row.mailbox)
    }
    const isMultiAccount = accounts.length > 1

    return {
      list: (sourceId) => [...(bySource.get(sourceId)?.values() ?? [])],
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
  }, [accounts, rows, accountDirectory])
}
