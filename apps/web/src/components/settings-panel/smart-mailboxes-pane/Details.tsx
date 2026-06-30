import { useQuery } from '@tanstack/react-query'
import type {
  AccountOverview,
  AppSettings,
  SmartMailbox,
  SmartMailboxSummary,
} from '../../../api/types'
import { queryKeys } from '../../../queryKeys'
import { runtimeViews } from '../../../runtime/views'
import { SmartMailboxEditor } from '../SmartMailboxEditor'
import { SourceMailboxEditor } from '../SourceMailboxEditor'
import { FeedbackBanner } from '../shared'
import type { SmartMailboxEditorTarget } from '../types'
import type { MailboxEditorTarget } from '../SmartMailboxesPane'
import { SmartMailboxesEmptyState } from './EmptyState'

export function SmartMailboxDetail({
  target,
  editingSmartMailbox,
  editorKey,
  accounts,
  settings,
  onCreateMailbox,
  onSaved,
  onAutomationSettingsSaved,
  onDeleted,
}: {
  target: SmartMailboxEditorTarget
  editingSmartMailbox: SmartMailbox | SmartMailboxSummary | null
  editorKey: string
  accounts: AccountOverview[]
  settings: AppSettings | null
  onCreateMailbox: () => void
  onSaved: (mailbox: SmartMailbox) => Promise<void>
  onAutomationSettingsSaved: (settings: AppSettings) => Promise<void>
  onDeleted: (mailboxId: string) => Promise<void>
}) {
  if (target !== 'new' && !editingSmartMailbox) {
    return <SmartMailboxesEmptyState onCreateMailbox={onCreateMailbox} />
  }

  return (
    <SmartMailboxEditor
      key={editorKey}
      editorTarget={target}
      editingSmartMailbox={editingSmartMailbox}
      accounts={accounts}
      settings={settings}
      onSaved={onSaved}
      onAutomationSettingsSaved={onAutomationSettingsSaved}
      onDeleted={onDeleted}
    />
  )
}

export function SourceMailboxDetail({
  target,
  accounts,
  settings,
  onAutomationSettingsSaved,
}: {
  target: Extract<MailboxEditorTarget, { kind: 'source' }>
  accounts: AccountOverview[]
  settings: AppSettings | null
  onAutomationSettingsSaved: (settings: AppSettings) => Promise<void>
}) {
  const account =
    accounts.find((candidate) => candidate.id === target.accountId) ?? null
  const mailboxesQuery = useQuery({
    queryKey: queryKeys.mailboxes(target.accountId),
    queryFn: () => runtimeViews.mail.mailboxes(target.accountId),
    enabled: account !== null,
  })
  const mailboxes = mailboxesQuery.data ?? []
  const mailbox =
    mailboxes.find((candidate) => candidate.id === target.mailboxId) ?? null

  if (!account) {
    return (
      <FeedbackBanner tone="error">Account no longer exists.</FeedbackBanner>
    )
  }
  if (mailboxesQuery.isPending) {
    return <p className="text-[12px] text-muted-foreground">Loading mailbox.</p>
  }
  if (mailboxesQuery.error) {
    return (
      <FeedbackBanner tone="error">
        {mailboxesQuery.error.message}
      </FeedbackBanner>
    )
  }
  if (!mailbox) {
    return (
      <FeedbackBanner tone="error">Mailbox no longer exists.</FeedbackBanner>
    )
  }

  return (
    <SourceMailboxEditor
      account={account}
      mailbox={mailbox}
      mailboxes={mailboxes}
      settings={settings}
      onAutomationSettingsSaved={onAutomationSettingsSaved}
    />
  )
}
