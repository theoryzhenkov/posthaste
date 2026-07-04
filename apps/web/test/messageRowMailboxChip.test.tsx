import { describe, expect, it, mock } from 'bun:test'
import { render } from '@testing-library/react'

import type { EmailActions } from '../src/hooks/useEmailActions'
import type { MessageSummary } from '../src/api/types'
import { MessageRow } from '../src/components/MessageRow'
import {
  buildThreadListLayout,
  DEFAULT_COLUMNS,
  type ColumnId,
} from '../src/components/thread-list/columns'
import type { MailboxDirectory } from '../src/components/message-list/useMailboxDirectory'
import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

const message: MessageSummary = {
  id: 'message-1',
  sourceId: 'account-1',
  sourceName: 'Work',
  sourceThreadId: 'thread-1',
  conversationId: 'conversation-1',
  subject: 'Hello',
  fromName: 'Sender',
  fromEmail: 'sender@example.test',
  to: [],
  preview: 'Preview',
  receivedAt: '2026-05-31T00:00:00Z',
  hasAttachment: false,
  isRead: false,
  isFlagged: false,
  mailboxIds: ['mailbox-1'],
  keywords: [],
}

const actions: EmailActions = {
  toggleRead: mock(() => {}),
  markRead: mock(() => {}),
  toggleFlag: mock(() => {}),
  setUserTags: mock(() => {}),
  archive: mock(() => {}),
  trash: mock(() => {}),
  discardDraft: mock(() => {}),
  moveToInbox: mock(() => {}),
  deletePermanently: mock(() => {}),
  clearError: mock(() => {}),
  errorMessage: null,
  isPending: false,
}

/** Columns including the (default-hidden) `sourceMailbox` column, so its cell
 *  renderer is exercised. */
const COLUMNS_WITH_MAILBOX: ColumnId[] = [...DEFAULT_COLUMNS, 'sourceMailbox']

function directoryResolvingTo(
  resolved: ReturnType<MailboxDirectory['resolve']>,
): MailboxDirectory {
  return { resolve: () => resolved }
}

function renderRow(overrides: {
  columns: ColumnId[]
  mailboxDirectory: MailboxDirectory
}) {
  return render(
    <MessageRow
      message={message}
      isSelected={false}
      isStriped={false}
      onSelectMessage={() => {}}
      columns={overrides.columns}
      layout={buildThreadListLayout(overrides.columns)}
      actions={actions}
      viewRole={null}
      onViewConversation={() => {}}
      excludeMailboxId={null}
      mailboxDirectory={overrides.mailboxDirectory}
    />,
  )
}

describe('sourceMailbox column cell', () => {
  it('renders the resolved mailbox name (and role icon) when the column is active', () => {
    const { queryByText } = renderRow({
      columns: COLUMNS_WITH_MAILBOX,
      mailboxDirectory: directoryResolvingTo({
        mailbox: {
          id: 'mailbox-1',
          name: 'Archive',
          role: 'archive',
          unreadEmails: 0,
          totalEmails: 0,
        },
        isMultiAccount: false,
        accountName: 'Work',
      }),
    })
    expect(queryByText('Archive')).not.toBeNull()
  })

  it('prefixes with the account name in multi-account views', () => {
    const { queryByText } = renderRow({
      columns: COLUMNS_WITH_MAILBOX,
      mailboxDirectory: directoryResolvingTo({
        mailbox: {
          id: 'mailbox-1',
          name: 'Inbox',
          role: 'inbox',
          unreadEmails: 0,
          totalEmails: 0,
        },
        isMultiAccount: true,
        accountName: 'Work',
      }),
    })
    expect(queryByText('Work · Inbox')).not.toBeNull()
  })

  it('renders no chip when the column is not active, even if a mailbox is resolvable', () => {
    const { queryByText } = renderRow({
      columns: DEFAULT_COLUMNS,
      mailboxDirectory: directoryResolvingTo({
        mailbox: {
          id: 'mailbox-1',
          name: 'Archive',
          role: 'archive',
          unreadEmails: 0,
          totalEmails: 0,
        },
        isMultiAccount: false,
        accountName: 'Work',
      }),
    })
    expect(queryByText('Archive')).toBeNull()
  })

  it('renders no chip (and does not crash) when the mailbox is unresolvable', () => {
    const { queryByText, getByText } = renderRow({
      columns: COLUMNS_WITH_MAILBOX,
      mailboxDirectory: directoryResolvingTo(null),
    })
    expect(queryByText('Archive')).toBeNull()
    // The row itself still renders fine.
    expect(getByText('Hello')).not.toBeNull()
  })
})
