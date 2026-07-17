/**
 * Source mailbox detail editor for server metadata and mailbox-scoped actions.
 *
 */
import type { AccountRow } from '@/gen'
import type {
  AppSettings,
  KnownMailboxRole,
  Mailbox,
} from '../../api/types'
import { isKnownMailboxRole, renderMailboxRoleIcon } from '../../mailboxRoles'
import { accentColor } from '@/design'
import { hueGradient } from './appearance/constants'
import { SourceMailboxAutomationFields } from './AutomationActionsEditor'
import { FeedbackBanner, SettingsPageHeader, SettingsSection } from './shared'
import { useMailboxColorMutation } from './useMailboxColorMutation'
import { useMailboxRoleMutation } from './useMailboxRoleMutation'
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
  { value: 'snooze', label: 'Snoozed' },
]

export function SourceMailboxEditor({
  account,
  mailbox,
  mailboxes,
  settings,
  onAutomationSettingsSaved,
}: {
  account: AccountRow
  mailbox: Mailbox
  mailboxes: Mailbox[]
  settings: AppSettings | null
  onAutomationSettingsSaved: (settings: AppSettings) => Promise<void>
}) {
  const roleMutation = useMailboxRoleMutation(account.id, mailbox.id)
  const colorMutation = useMailboxColorMutation()
  const mailboxColors = settings?.mailboxColors ?? []
  const colorHue = mailboxColors.find(
    (entry) => entry.sourceId === account.id && entry.mailboxId === mailbox.id,
  )?.hue
  const setColorHue = (hue: number | null) => {
    const without = mailboxColors.filter(
      (entry) =>
        !(entry.sourceId === account.id && entry.mailboxId === mailbox.id),
    )
    colorMutation.mutate(
      hue == null
        ? without
        : [...without, { sourceId: account.id, mailboxId: mailbox.id, hue }],
    )
  }
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

      <SettingsSection title="Sidebar color">
        <div className="flex flex-col gap-3">
          <p className="text-[12px] leading-5 text-muted-foreground">
            Override this mailbox's sidebar color. Defaults to its role color.
          </p>
          <div className="flex items-center gap-3">
            <span
              className="size-7 shrink-0 rounded-md border border-border-soft"
              style={{
                backgroundColor: accentColor(colorHue ?? 0),
                opacity: colorHue == null ? 0.3 : 1,
              }}
            />
            <input
              type="range"
              min={0}
              max={359}
              step={1}
              value={colorHue ?? 0}
              onChange={(event) => setColorHue(Number(event.target.value))}
              aria-label="Mailbox color hue"
              className="ph-hue-range h-4 flex-1 cursor-pointer appearance-none rounded-full border border-border-soft bg-transparent accent-primary"
              style={{ background: hueGradient }}
            />
            <button
              type="button"
              disabled={colorHue == null}
              onClick={() => setColorHue(null)}
              className="ph-focus-ring h-7 rounded-md border border-border-soft px-2.5 text-[12px] font-medium text-muted-foreground transition-colors hover:bg-background/60 hover:text-foreground disabled:opacity-35"
            >
              Default
            </button>
          </div>
        </div>
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
