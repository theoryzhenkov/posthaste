/**
 * Muted chip showing which mailbox a message row lives in, for the "show
 * source mailbox" toggle — most useful in aggregate views (unified inbox,
 * smart mailboxes, search) where rows come from different mailboxes/accounts.
 * Styled to match `TagChip`'s pill convention, with the mailbox's role icon
 * from the same lookup the sidebar uses (`renderMailboxRoleIcon`).
 *
 */
import { cn } from '@/lib/design/cn'
import { renderMailboxRoleIcon } from '@/domain/role'

export function MailboxChip({
  name,
  role,
  accountName,
  className,
}: {
  name: string
  role: string | null
  /** Account short name to prefix with in unified/multi-account views; null
   *  when the view is single-account. */
  accountName?: string | null
  className?: string
}) {
  return (
    <span
      className={cn(
        'inline-flex h-5 max-w-full shrink-0 items-center gap-1 rounded-full bg-[var(--bg-elev)] px-2 text-[11px] font-medium text-muted-foreground',
        className,
      )}
    >
      {renderMailboxRoleIcon(role, 11)}
      <span className="min-w-0 truncate">
        {accountName ? `${accountName} · ${name}` : name}
      </span>
    </span>
  )
}
