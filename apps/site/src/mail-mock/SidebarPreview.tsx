import { useState } from 'react'
import { Archive, ChevronDown, Inbox } from 'lucide-react'
import type { Mailbox, MailboxView, mailboxCounts } from './types'

export function SidebarPreview({
  counts,
  selectedMailbox,
  onSelectMailbox,
}: {
  counts: ReturnType<typeof mailboxCounts>
  selectedMailbox: MailboxView
  onSelectMailbox: (mailbox: MailboxView) => void
}) {
  const [isExpanded, setIsExpanded] = useState(true)
  const mailboxes: Mailbox[] = [
    {
      id: 'inbox',
      label: 'Inbox',
      count: String(counts.inbox),
      active: selectedMailbox === 'inbox',
    },
    {
      id: 'archive',
      label: 'Archive',
      count: String(counts.archive),
      active: selectedMailbox === 'archive',
    },
  ]

  return (
    <aside className="mock-sidebar" aria-label="Mailbox preview">
      <SidebarSection label="Accounts" />
      <div className="source-section">
        <button
          type="button"
          className="source-header"
          aria-expanded={isExpanded}
          onClick={() => setIsExpanded((current) => !current)}
        >
          <ChevronDown aria-hidden="true" />
          <span className="account-stamp stalwart">S</span>
          <span>Stalwart</span>
          {counts.inbox > 0 ? (
            <span className="source-count">{counts.inbox}</span>
          ) : null}
        </button>
        {isExpanded
          ? mailboxes.map((mailbox) => (
              <SidebarItem
                mailbox={mailbox}
                key={mailbox.id}
                onSelect={() => onSelectMailbox(mailbox.id)}
              />
            ))
          : null}
      </div>
    </aside>
  )
}

function SidebarSection({ label }: { label: string }) {
  return <div className="section-label">{label}</div>
}

function SidebarItem({
  mailbox,
  onSelect,
}: {
  mailbox: Mailbox
  onSelect: () => void
}) {
  const Icon = mailbox.id === 'archive' ? Archive : Inbox

  return (
    <button
      type="button"
      className={`mailbox-row nested ${mailbox.active ? 'active' : ''}`}
      onClick={onSelect}
    >
      <Icon aria-hidden="true" />
      <span>{mailbox.label}</span>
      {Number(mailbox.count) > 0 ? (
        <span className="count">{mailbox.count}</span>
      ) : null}
    </button>
  )
}
