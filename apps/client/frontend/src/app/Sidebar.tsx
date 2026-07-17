// The sidebar pane: the all-mail smart view, then each account with its
// mailboxes and unread counts, then a quiet pending-operations footer. Every
// count and status dot renders a query answer; the only local state is which
// account sections are collapsed.

import { useState } from 'react'
import type { AccountRow, MailboxCountsRow, PendingOperationRow } from '../gen'
import { viewKey, viewForSidebarRow, type Pane, type View } from './model'

/** Partitions the mailbox-counts answer by account for rendering under the
 * account headers. The rows arrive from the backend already in display order
 * (role precedence, then name), so this is a single pass that preserves that
 * order — the client never re-sorts a query answer. */
export function groupMailboxRows(rows: MailboxCountsRow[]): Map<string, MailboxCountsRow[]> {
  const grouped = new Map<string, MailboxCountsRow[]>()
  for (const row of rows) {
    const group = grouped.get(row.accountId)
    if (group) group.push(row)
    else grouped.set(row.accountId, [row])
  }
  return grouped
}

function statusDotClass(account: AccountRow): string {
  switch (account.status) {
    case 'ready':
      return 'status-dot ok'
    case 'syncing':
      return 'status-dot busy'
    case 'disabled':
      return 'status-dot off'
    default:
      return 'status-dot bad'
  }
}

export function Sidebar({
  accounts,
  mailboxes,
  pending,
  view,
  activePane,
  onSelectView,
  onActivate,
}: {
  accounts: AccountRow[]
  mailboxes: MailboxCountsRow[]
  pending: PendingOperationRow[]
  view: View
  activePane: Pane
  onSelectView: (view: View) => void
  onActivate: () => void
}) {
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set())
  const currentKey = viewKey(view)
  const active = activePane === 'sidebar'
  const grouped = groupMailboxRows(mailboxes)

  const toggleCollapsed = (accountId: string) => {
    setCollapsed((prev) => {
      const next = new Set(prev)
      if (next.has(accountId)) next.delete(accountId)
      else next.add(accountId)
      return next
    })
  }

  const inFlight = pending.filter(
    (op) => op.state === 'pending' || op.state === 'inflight' || op.state === 'dispatchUncertain',
  ).length
  const failed = pending.filter((op) => op.state === 'failed').length

  return (
    <nav
      className="sidebar"
      data-active={active || undefined}
      onPointerDown={onActivate}
      aria-label="Mailboxes"
    >
      <div className="sidebar-scroll">
        <button
          type="button"
          className="sidebar-row"
          data-selected={currentKey === 'all' || undefined}
          onClick={() => onSelectView({ kind: 'all' })}
        >
          <span className="sidebar-row-name">All Mail</span>
        </button>

        {accounts.map((account) => {
          const boxes = grouped.get(account.id) ?? []
          const isCollapsed = collapsed.has(account.id)
          return (
            <section key={account.id} className="sidebar-account">
              <button
                type="button"
                className="sidebar-account-header"
                onClick={() => toggleCollapsed(account.id)}
                title={account.lastSyncError ?? account.status}
              >
                <span className={statusDotClass(account)} aria-hidden />
                <span className="sidebar-account-name">{account.name}</span>
                <span className="sidebar-chevron">{isCollapsed ? '▸' : '▾'}</span>
              </button>
              {!isCollapsed &&
                boxes.map((row) => {
                  const rowView = viewForSidebarRow({
                    type: 'mailbox',
                    accountId: row.accountId,
                    mailbox: row.mailbox,
                  })
                  return (
                    <button
                      key={row.mailbox.id}
                      type="button"
                      className="sidebar-row indent"
                      data-selected={viewKey(rowView) === currentKey || undefined}
                      onClick={() => onSelectView(rowView)}
                    >
                      <span className="sidebar-row-name">{row.mailbox.name}</span>
                      {row.mailbox.unreadEmails > 0 && (
                        <span className="sidebar-count">{row.mailbox.unreadEmails}</span>
                      )}
                    </button>
                  )
                })}
            </section>
          )
        })}
      </div>

      {(inFlight > 0 || failed > 0) && (
        <footer className="sidebar-footer" title="Pending operations">
          {inFlight > 0 && <span className="pending-chip">{inFlight} pending</span>}
          {failed > 0 && <span className="pending-chip failed">{failed} failed</span>}
        </footer>
      )}
    </nav>
  )
}
