/**
 * Source mailbox detail editor for server metadata and mailbox-scoped actions.
 *
 * @spec docs/L1-api#mailbox-metadata
 * @spec docs/L1-ui#account-settings
 */
import { useMutation, useQueryClient } from '@tanstack/react-query'
import type {
  AccountOverview,
  AppSettings,
  KnownMailboxRole,
  Mailbox,
} from '../../api/types'
import { patchMailbox } from '../../api/client'
import { invalidateAccountReadModels } from '../../domainCache'
import { isKnownMailboxRole, renderMailboxRoleIcon } from '../../mailboxRoles'
import { queryKeys } from '../../queryKeys'
import { SourceMailboxAutomationFields } from './AutomationActionsEditor'
import { FeedbackBanner, SettingsPageHeader, SettingsSection } from './shared'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '../ui/select'

const mailboxRoleOptions: Array<{
  value: KnownMailboxRole | '__none__'
  label: string
}> = [
  { value: '__none__', label: 'None' },
  { value: 'inbox', label: 'Inbox' },
  { value: 'archive', label: 'Archive' },
  { value: 'drafts', label: 'Drafts' },
  { value: 'sent', label: 'Sent' },
  { value: 'junk', label: 'Junk' },
  { value: 'trash', label: 'Trash' },
]

export function SourceMailboxEditor({
  account,
  mailbox,
  mailboxes,
  settings,
  onAutomationSettingsSaved,
}: {
  account: AccountOverview
  mailbox: Mailbox
  mailboxes: Mailbox[]
  settings: AppSettings | null
  onAutomationSettingsSaved: (settings: AppSettings) => Promise<void>
}) {
  const queryClient = useQueryClient()
  const roleMutation = useMutation({
    mutationFn: (role: KnownMailboxRole | null) =>
      patchMailbox(account.id, mailbox.id, { role }),
    onSuccess: (nextMailboxes) => {
      queryClient.setQueryData(queryKeys.mailboxes(account.id), nextMailboxes)
      invalidateAccountReadModels(queryClient, account.id)
    },
  })
  const hasUnknownRole = Boolean(
    mailbox.role && !isKnownMailboxRole(mailbox.role),
  )
  const selectValue =
    mailbox.role && (isKnownMailboxRole(mailbox.role) || hasUnknownRole)
      ? mailbox.role
      : '__none__'

  return (
    <div className="pb-8">
      <SettingsPageHeader
        title={mailbox.name}
        meta={
          <p className="text-[13px] text-muted-foreground">
            {account.name} · {mailbox.totalEmails} messages ·{' '}
            {mailbox.unreadEmails} unread
          </p>
        }
        leading={
          <span className="flex size-10 shrink-0 items-center justify-center rounded-md bg-muted text-muted-foreground">
            {renderMailboxRoleIcon(mailbox.role, 18)}
          </span>
        }
      />

      <SettingsSection title="Definition">
        <label className="grid gap-1.5 text-[13px]">
          <span className="text-[12px] font-medium text-muted-foreground">
            Server role
          </span>
          <Select
            value={selectValue}
            disabled={roleMutation.isPending}
            onValueChange={(value) =>
              roleMutation.mutate(
                value === '__none__' ? null : (value as KnownMailboxRole),
              )
            }
          >
            <SelectTrigger className="h-8 w-full rounded-md border-border bg-background text-[13px] shadow-none">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {hasUnknownRole && mailbox.role && (
                <SelectItem value={mailbox.role}>
                  Unknown: {mailbox.role}
                </SelectItem>
              )}
              {mailboxRoleOptions.map((option) => (
                <SelectItem key={option.value} value={option.value}>
                  {option.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </label>

        {roleMutation.error && (
          <FeedbackBanner tone="error">
            {roleMutation.error.message}
          </FeedbackBanner>
        )}
      </SettingsSection>

      <SettingsSection title="Actions">
        {settings ? (
          <SourceMailboxAutomationFields
            key={`${account.id}:${mailbox.id}`}
            account={account}
            mailbox={mailbox}
            mailboxes={mailboxes}
            settings={settings}
            onSaved={onAutomationSettingsSaved}
          />
        ) : (
          <p className="text-[12px] text-muted-foreground">
            Settings are still loading.
          </p>
        )}
      </SettingsSection>
    </div>
  )
}
