// Shell-level ephemera: the effective view, the selection, and the pane
// focus model. All of it is component state — which view is shown and which
// message is open — never mail data; everything mail-shaped renders from
// query answers.

import type { AccountId, MailboxId, MailboxSummary, MessageId, MessageSummary, ThreadId } from '../gen'

/** What the list pane shows: all mail across accounts, or one account's
 * mailbox. */
export type View =
  | { kind: 'all' }
  | { kind: 'mailbox'; accountId: AccountId; mailboxId: MailboxId; name: string; role: string | null }

/** Stable identity for comparing views (sidebar highlight, scope resets). */
export function viewKey(view: View): string {
  return view.kind === 'all' ? 'all' : `mailbox:${view.accountId}:${view.mailboxId}`
}

/** The open message: ids only — the thread and detail queries render it. */
export interface Selection {
  accountId: AccountId
  messageId: MessageId
  threadId: ThreadId
}

export function selectionFor(row: MessageSummary): Selection {
  return { accountId: row.sourceId, messageId: row.id, threadId: row.sourceThreadId }
}

/** The keyboard-focusable panes. The detail pane is not focusable: it only
 * displays the list's selected message, and `j`/`k` in the list drive it. */
export type Pane = 'sidebar' | 'list'

/** One selectable sidebar row (account headers are not selectable). */
export type SidebarRow =
  | { type: 'all' }
  | { type: 'mailbox'; accountId: AccountId; mailbox: MailboxSummary }

export function viewForSidebarRow(row: SidebarRow): View {
  if (row.type === 'all') return { kind: 'all' }
  return {
    kind: 'mailbox',
    accountId: row.accountId,
    mailboxId: row.mailbox.id,
    name: row.mailbox.name,
    role: row.mailbox.role,
  }
}
