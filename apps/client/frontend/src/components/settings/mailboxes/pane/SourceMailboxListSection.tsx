import type { AccountRow } from '@/gen'

import { mailboxRoleAccent, renderMailboxRoleIcon } from '../../../../domain/mailboxRoles'
import { useMailboxCounts } from '@/data'
import { SettingsList } from '../../panel/shared'
import { MailboxListRow } from './MailboxListRow'

export function SourceMailboxListSection({
  account,
  onSelectMailbox,
}: {
  account: AccountRow
  onSelectMailbox: (mailboxId: string) => void
}) {
  const mailboxesQuery = useMailboxCounts(account.id)
  const mailboxes = mailboxesQuery.data?.rows.map((row) => row.mailbox) ?? []

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
