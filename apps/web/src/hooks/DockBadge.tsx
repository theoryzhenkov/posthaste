/**
 * App-root Dock/taskbar unread-badge driver. Sums inbox unread across enabled
 * accounts and pushes it to the app icon (see useDockBadge.ts for the count
 * selection + Tauri sink). Renders nothing; mount once in the main window.
 */
import { useQuery } from '@tanstack/react-query'
import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from 'react'

import type { AccountOverview, Mailbox } from '@/api/types'
import { queryKeys } from '@/queryKeys'
import { runtimeViews } from '@/runtime/views'

import { accountInboxUnread, useDockBadge } from './useDockBadge'

/**
 * One account's live inbox-unread reporter. A cache-only observer of the
 * mailbox read-model (populated + kept fresh by the sidebar's
 * `useMailboxNavigationReadModels`, invalidated on count-affecting events, and
 * adjusted by the optimistic overlay), re-reporting its total up when the rows
 * move. Per-account because a hook cannot subscribe to a variable-length
 * account list in one call.
 */
function AccountInboxUnreadProbe({
  accountId,
  onReport,
}: {
  accountId: string
  onReport: (accountId: string, unread: number) => void
}): null {
  const { data: mailboxes } = useQuery<Mailbox[]>({
    queryKey: queryKeys.mailboxes(accountId),
    queryFn: () => runtimeViews.mail.mailboxes(accountId),
    enabled: false,
  })
  const unread = useMemo(() => accountInboxUnread(mailboxes ?? []), [mailboxes])
  useEffect(() => {
    onReport(accountId, unread)
  }, [accountId, unread, onReport])
  return null
}

/**
 * App-root badge driver. Sums inbox unread across enabled accounts and pushes it
 * to the app icon. Renders nothing; mount once in the main window.
 */
export function DockBadge(): ReactNode {
  const { data: accounts } = useQuery<AccountOverview[]>({
    queryKey: queryKeys.accounts,
    queryFn: runtimeViews.accounts.list,
    enabled: false,
  })
  const enabledAccountIds = useMemo(
    () =>
      (accounts ?? []).filter((account) => account.enabled).map((a) => a.id),
    [accounts],
  )

  const [unreadByAccount, setUnreadByAccount] = useState<
    Record<string, number>
  >({})
  const reportCount = useCallback((accountId: string, unread: number) => {
    setUnreadByAccount((prev) =>
      prev[accountId] === unread ? prev : { ...prev, [accountId]: unread },
    )
  }, [])

  const total = useMemo(
    () =>
      enabledAccountIds.reduce(
        (sum, id) => sum + (unreadByAccount[id] ?? 0),
        0,
      ),
    [enabledAccountIds, unreadByAccount],
  )
  useDockBadge(total)

  return enabledAccountIds.map((accountId) => (
    <AccountInboxUnreadProbe
      key={accountId}
      accountId={accountId}
      onReport={reportCount}
    />
  ))
}
