/**
 * Smart mailboxes view: centered list with drill-in mailbox editing.
 *
 * @spec docs/L1-api#smart-mailbox-crud
 */
import { FolderSearch, Plus } from 'lucide-react'
import type { SmartMailbox, SmartMailboxSummary } from '../../api/types'
import { brandAccents } from '../../design/tokens'
import { Button } from '../ui/button'
import { SmartMailboxEditor } from './SmartMailboxEditor'
import {
  SettingsBackButton,
  SettingsEmptyState,
  SettingsList,
  SettingsPage,
  SettingsPageHeader,
} from './shared'
import type { SmartMailboxEditorTarget } from './types'

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
  selectedMailboxId,
  editingSmartMailbox,
  editorKey,
  actionPendingKey,
  actionError,
  onSelectMailbox,
  onBackToMailboxes,
  onCreateMailbox,
  onResetDefaults,
  onReorderMailbox,
  onSaved,
  onDeleted,
}: {
  smartMailboxes: SmartMailboxSummary[]
  selectedMailboxId: SmartMailboxEditorTarget | null
  editingSmartMailbox: SmartMailbox | SmartMailboxSummary | null
  editorKey: string
  actionPendingKey: string | null
  actionError: string | null
  onSelectMailbox: (mailboxId: string) => void
  onBackToMailboxes: () => void
  onCreateMailbox: () => void
  onResetDefaults: () => void
  onReorderMailbox: (mailbox: SmartMailboxSummary, position: number) => void
  onSaved: (mailbox: SmartMailbox) => Promise<void>
  onDeleted: (mailboxId: string) => Promise<void>
}) {
  const selectedMailbox =
    selectedMailboxId === null || selectedMailboxId === 'new'
      ? null
      : (smartMailboxes.find((mailbox) => mailbox.id === selectedMailboxId) ??
        null)

  if (selectedMailboxId !== null) {
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
            <p className="mb-4 rounded-md border border-destructive/20 bg-destructive/5 px-3 py-2 text-[12px] text-destructive">
              {actionError}
            </p>
          )}

          {selectedMailboxId === 'new' || editingSmartMailbox ? (
            <SmartMailboxEditor
              key={editorKey}
              editorTarget={selectedMailboxId}
              editingSmartMailbox={editingSmartMailbox}
              summary={selectedMailbox}
              onSaved={onSaved}
              onDeleted={onDeleted}
              onReorder={onReorderMailbox}
              reorderPendingKey={actionPendingKey}
            />
          ) : (
            <SmartMailboxesEmptyState onCreateMailbox={onCreateMailbox} />
          )}
        </SettingsPage>
      </section>
    )
  }

  return (
    <section className="ph-scroll h-full min-h-0 overflow-y-auto px-6 py-8">
      <SettingsPage>
        <SettingsPageHeader
          title="Smart mailboxes"
          description="Saved views filter messages into focused mailboxes without changing source accounts. Use them for inboxes, rules, and repeat workflows."
        />

        {actionError && (
          <p className="mt-6 rounded-lg border border-destructive/20 bg-destructive/5 px-3 py-2 text-[12px] text-destructive">
            {actionError}
          </p>
        )}

        {smartMailboxes.length === 0 ? (
          <div className="mt-10">
            <SmartMailboxesEmptyState onCreateMailbox={onCreateMailbox} />
          </div>
        ) : (
          <SettingsList
            title={`${smartMailboxes.length} saved ${
              smartMailboxes.length === 1 ? 'view' : 'views'
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
            {smartMailboxes.map((mailbox) => (
              <MailboxListRow
                key={mailbox.id}
                accent={smartMailboxAccent(mailbox.name)}
                label={mailbox.name}
                sublabel={`${mailbox.totalMessages} messages · ${mailbox.unreadMessages} unread`}
                isDefault={mailbox.kind === 'default'}
                onClick={() => onSelectMailbox(mailbox.id)}
              />
            ))}
          </SettingsList>
        )}
      </SettingsPage>
    </section>
  )
}

function MailboxListRow({
  accent,
  label,
  sublabel,
  isDefault,
  onClick,
}: {
  accent: string
  label: string
  sublabel?: string
  isDefault?: boolean
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
        <FolderSearch size={15} strokeWidth={1.45} />
      </span>
      <span className="min-w-0 flex-1">
        <span className="flex items-center gap-2">
          <span className="truncate text-[13px] font-medium text-foreground">
            {label}
          </span>
          {isDefault && (
            <span
              className="shrink-0 rounded-sm bg-background/80 px-1.5 py-0.5 font-mono text-[9px] uppercase tracking-[0.18em] text-muted-foreground"
              title="Built-in smart mailbox"
            >
              default
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
      icon={<FolderSearch size={36} strokeWidth={1.5} />}
      title="No smart mailboxes yet"
      description="Create a saved view to keep important mail easy to find."
      action={
        <Button
          size="sm"
          variant="outline"
          type="button"
          onClick={onCreateMailbox}
          className="rounded-md border-border bg-bg-elev"
        >
          <Plus size={13} strokeWidth={2} />
          New mailbox
        </Button>
      }
    />
  )
}
