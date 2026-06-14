import { useQuery } from '@tanstack/react-query'

import { fetchMailboxes } from '../../../api/client'
import type { AccountOverview } from '../../../api/types'
import { mailboxRoleAccent, renderMailboxRoleIcon } from '../../../mailboxRoles'
import { queryKeys } from '../../../queryKeys'
import { SettingsList } from '../shared'
import { MailboxListRow } from './MailboxListRow'

export function SourceMailboxListSection({
  account,
  onSelectMailbox,
}: {
  account: AccountOverview
  onSelectMailbox: (mailboxId: string) => void
}) {
  const mailboxesQuery = useQuery({
    queryKey: queryKeys.mailboxes(account.id),
    queryFn: () => fetchMailboxes(account.id),
  })
  const mailboxes = mailboxesQuery.data ?? []

  return (
    <SettingsList title={account.name}>
      {mailboxesQuery.isPending ? (
        <p className="px-4 py-3 text-[12px] text-muted-foreground">
          Loading mailboxes.
        </p>
      ) : mailboxesQuery.error ? (
        <p className="px-4 py-3 text-[12px] text-destructive">
          {mailboxesQuery.error.message}
        </p>
      ) : mailboxes.length === 0 ? (
        <p className="px-4 py-3 text-[12px] text-muted-foreground">
          No synced mailboxes yet.
        </p>
      ) : (
        mailboxes.map((mailbox) => (
          <MailboxListRow
            key={mailbox.id}
            accent={mailboxRoleAccent(mailbox.role)}
            icon={renderMailboxRoleIcon(mailbox.role, 15)}
            label={mailbox.name}
            sublabel={`${mailbox.totalEmails} messages · ${mailbox.unreadEmails} unread`}
            badge={mailbox.role}
            onClick={() => onSelectMailbox(mailbox.id)}
          />
        ))
      )}
    </SettingsList>
  )
}
