/**
 * Unified mailbox settings view with drill-in editors for smart and source mailboxes.
 *
 * @spec docs/L1-api#smart-mailbox-crud
 * @spec docs/L1-api#mailbox-metadata
 */
import { useQuery } from '@tanstack/react-query'
import type React from 'react'
import { Folder, FolderSearch, Plus } from 'lucide-react'
import type {
  AccountOverview,
  AppSettings,
  SmartMailbox,
  SmartMailboxSummary,
} from '../../api/types'
import { fetchMailboxes } from '../../api/client'
import { brandAccents } from '../../design/tokens'
import { renderMailboxRoleIcon } from '../../mailboxRoles'
import { queryKeys } from '../../queryKeys'
import { Button } from '../ui/button'
import { SmartMailboxEditor } from './SmartMailboxEditor'
import { SourceMailboxEditor } from './SourceMailboxEditor'
import {
  FeedbackBanner,
  SettingsBackButton,
  SettingsEmptyState,
  SettingsList,
  SettingsPage,
  SettingsPageHeader,
} from './shared'
import type { SmartMailboxEditorTarget } from './types'

export type MailboxEditorTarget =
  | { kind: 'smart'; id: SmartMailboxEditorTarget }
  | { kind: 'source'; accountId: string; mailboxId: string }

const MAILBOX_ACCENTS = {
  blue: brandAccents.blue,
  coral: brandAccents.coral,
  sage: brandAccents.sage,
  amber: brandAccents.amber,
  violet: brandAccents.violet,
  rose: brandAccents.rose,
  muted: 'oklch(0.60 0.008 70)',
} as const

function smartMailboxAccent(name: string): string {
  const normalized = name.trim().toLowerCase()
  switch (normalized) {
    case 'inbox':
    case 'all inboxes':
    case 'all mail':
    case 'today':
    case 'archive':
    case 'work':
      return MAILBOX_ACCENTS.blue
    case 'flagged':
    case 'relevant':
    case 'sent':
    case 'follow-up':
      return MAILBOX_ACCENTS.coral
    case 'read later':
    case 'read-later':
    case 'junk':
    case 'spam':
      return MAILBOX_ACCENTS.amber
    case 'bills':
    case 'billing':
    case 'drafts':
      return MAILBOX_ACCENTS.violet
    case 'newsletters':
    case 'personal':
      return MAILBOX_ACCENTS.sage
    case 'trash':
      return MAILBOX_ACCENTS.rose
    default:
      return MAILBOX_ACCENTS.muted
  }
}

export function SmartMailboxesPane({
  smartMailboxes,
  accounts,
  settings,
  selectedMailboxTarget,
  editingSmartMailbox,
  editorKey,
  actionPendingKey,
  actionError,
  onSelectSmartMailbox,
  onSelectSourceMailbox,
  onBackToMailboxes,
  onCreateMailbox,
  onResetDefaults,
  onReorderMailbox,
  onSaved,
  onAutomationSettingsSaved,
  onDeleted,
}: {
  smartMailboxes: SmartMailboxSummary[]
  accounts: AccountOverview[]
  settings: AppSettings | null
  selectedMailboxTarget: MailboxEditorTarget | null
  editingSmartMailbox: SmartMailbox | SmartMailboxSummary | null
  editorKey: string
  actionPendingKey: string | null
  actionError: string | null
  onSelectSmartMailbox: (mailboxId: string) => void
  onSelectSourceMailbox: (accountId: string, mailboxId: string) => void
  onBackToMailboxes: () => void
  onCreateMailbox: () => void
  onResetDefaults: () => void
  onReorderMailbox: (mailbox: SmartMailboxSummary, position: number) => void
  onSaved: (mailbox: SmartMailbox) => Promise<void>
  onAutomationSettingsSaved: (settings: AppSettings) => Promise<void>
  onDeleted: (mailboxId: string) => Promise<void>
}) {
  if (selectedMailboxTarget !== null) {
    return (
      <section className="ph-scroll h-full min-h-0 overflow-y-auto px-6 py-8">
        <SettingsPage>
          <SettingsBackButton
            ariaLabel="Back to mailboxes"
            onClick={onBackToMailboxes}
          >
            Mailboxes & Rules
          </SettingsBackButton>

          {actionError && (
            <FeedbackBanner tone="error">{actionError}</FeedbackBanner>
          )}

          {selectedMailboxTarget.kind === 'smart' ? (
            <SmartMailboxDetail
              target={selectedMailboxTarget.id}
              smartMailboxes={smartMailboxes}
              editingSmartMailbox={editingSmartMailbox}
              editorKey={editorKey}
              accounts={accounts}
              settings={settings}
              actionPendingKey={actionPendingKey}
              onCreateMailbox={onCreateMailbox}
              onSaved={onSaved}
              onAutomationSettingsSaved={onAutomationSettingsSaved}
              onDeleted={onDeleted}
              onReorderMailbox={onReorderMailbox}
            />
          ) : (
            <SourceMailboxDetail
              target={selectedMailboxTarget}
              accounts={accounts}
              settings={settings}
              onAutomationSettingsSaved={onAutomationSettingsSaved}
            />
          )}
        </SettingsPage>
      </section>
    )
  }

  return (
    <section className="ph-scroll h-full min-h-0 overflow-y-auto px-6 py-8">
      <SettingsPage>
        <SettingsPageHeader
          title="Mailboxes & Rules"
          description="Edit smart views and source mailboxes from one focused place. Mailbox actions run on the backend."
        />

        {actionError && (
          <p className="mt-6 rounded-lg border border-destructive/20 bg-destructive/5 px-3 py-2 text-[12px] text-destructive">
            {actionError}
          </p>
        )}

        <SettingsList
          title={`${smartMailboxes.length} smart ${
            smartMailboxes.length === 1 ? 'mailbox' : 'mailboxes'
          }`}
          actions={
            <>
              <Button
                size="sm"
                variant="outline"
                type="button"
                className="h-7 rounded-[5px] border-border bg-background px-2 text-[12px]"
                onClick={onResetDefaults}
                disabled={actionPendingKey !== null}
              >
                Reset defaults
              </Button>
              <Button
                aria-label="New smart mailbox"
                size="icon-sm"
                variant="outline"
                type="button"
                onClick={onCreateMailbox}
                className="size-7 rounded-[5px] border-border bg-background text-muted-foreground hover:text-foreground"
              >
                <Plus size={14} strokeWidth={1.8} />
              </Button>
            </>
          }
        >
          {smartMailboxes.length === 0 ? (
            <div className="p-4">
              <SmartMailboxesEmptyState onCreateMailbox={onCreateMailbox} />
            </div>
          ) : (
            smartMailboxes.map((mailbox) => (
              <MailboxListRow
                key={mailbox.id}
                accent={smartMailboxAccent(mailbox.name)}
                icon={<FolderSearch size={15} strokeWidth={1.45} />}
                label={mailbox.name}
                sublabel={`${mailbox.totalMessages} messages · ${mailbox.unreadMessages} unread`}
                badge={mailbox.kind === 'default' ? 'default' : null}
                onClick={() => onSelectSmartMailbox(mailbox.id)}
              />
            ))
          )}
        </SettingsList>

        {accounts.map((account) => (
          <SourceMailboxListSection
            key={account.id}
            account={account}
            onSelectMailbox={(mailboxId) =>
              onSelectSourceMailbox(account.id, mailboxId)
            }
          />
        ))}
      </SettingsPage>
    </section>
  )
}

function SmartMailboxDetail({
  target,
  smartMailboxes,
  editingSmartMailbox,
  editorKey,
  accounts,
  settings,
  actionPendingKey,
  onCreateMailbox,
  onSaved,
  onAutomationSettingsSaved,
  onDeleted,
  onReorderMailbox,
}: {
  target: SmartMailboxEditorTarget
  smartMailboxes: SmartMailboxSummary[]
  editingSmartMailbox: SmartMailbox | SmartMailboxSummary | null
  editorKey: string
  accounts: AccountOverview[]
  settings: AppSettings | null
  actionPendingKey: string | null
  onCreateMailbox: () => void
  onSaved: (mailbox: SmartMailbox) => Promise<void>
  onAutomationSettingsSaved: (settings: AppSettings) => Promise<void>
  onDeleted: (mailboxId: string) => Promise<void>
  onReorderMailbox: (mailbox: SmartMailboxSummary, position: number) => void
}) {
  const selectedMailbox =
    target === 'new'
      ? null
      : (smartMailboxes.find((mailbox) => mailbox.id === target) ?? null)

  if (target !== 'new' && !editingSmartMailbox) {
    return <SmartMailboxesEmptyState onCreateMailbox={onCreateMailbox} />
  }

  return (
    <SmartMailboxEditor
      key={editorKey}
      editorTarget={target}
      editingSmartMailbox={editingSmartMailbox}
      summary={selectedMailbox}
      accounts={accounts}
      settings={settings}
      onSaved={onSaved}
      onAutomationSettingsSaved={onAutomationSettingsSaved}
      onDeleted={onDeleted}
      onReorder={onReorderMailbox}
      reorderPendingKey={actionPendingKey}
    />
  )
}

function SourceMailboxDetail({
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
    queryFn: () => fetchMailboxes(target.accountId),
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

function SourceMailboxListSection({
  account,
  onSelectMailbox,
}: {
  account: AccountOverview
  onSelectMailbox: (mailboxId: string) => void
}) {
  const mailboxesQuery = useQuery({
    queryKey: queryKeys.mailboxes(account.id),
    queryFn: () => fetchMailboxes(account.id),
  })
  const mailboxes = mailboxesQuery.data ?? []

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
            accent={smartMailboxAccent(mailbox.role ?? mailbox.name)}
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

function MailboxListRow({
  accent,
  icon,
  label,
  sublabel,
  badge,
  onClick,
}: {
  accent: string
  icon: React.ReactNode
  label: string
  sublabel?: string
  badge?: string | null
  onClick: () => void
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="group flex min-h-[56px] w-full items-center gap-3 border-b border-border-soft px-4 text-left transition-colors last:border-b-0 hover:bg-[var(--list-hover)]"
    >
      <span
        className="flex size-8 shrink-0 items-center justify-center rounded-[5px] border"
        style={{
          backgroundColor: `color-mix(in oklab, ${accent} 14%, transparent)`,
          borderColor: `color-mix(in oklab, ${accent} 26%, transparent)`,
          color: accent,
        }}
      >
        {icon}
      </span>
      <span className="min-w-0 flex-1">
        <span className="flex items-center gap-2">
          <span className="truncate text-[13px] font-medium text-foreground">
            {label}
          </span>
          {badge && (
            <span
              className="shrink-0 rounded-sm bg-background/80 px-1.5 py-0.5 font-mono text-[9px] uppercase tracking-[0.18em] text-muted-foreground"
              title={badge}
            >
              {badge}
            </span>
          )}
        </span>
        {sublabel && (
          <span className="mt-0.5 block truncate text-[12px] text-muted-foreground">
            {sublabel}
          </span>
        )}
      </span>
      <span className="text-[12px] text-muted-foreground group-hover:text-foreground">
        Edit
      </span>
    </button>
  )
}

function SmartMailboxesEmptyState({
  onCreateMailbox,
}: {
  onCreateMailbox: () => void
}) {
  return (
    <SettingsEmptyState
      icon={<Folder size={36} strokeWidth={1.5} />}
      title="No mailbox selected"
      description="Pick a mailbox from the list, or create a smart mailbox."
      action={
        <Button
          size="sm"
          variant="outline"
          type="button"
          onClick={onCreateMailbox}
          className="rounded-md border-border bg-bg-elev"
        >
          <Plus size={13} strokeWidth={2} />
          New smart mailbox
        </Button>
      }
    />
  )
}
