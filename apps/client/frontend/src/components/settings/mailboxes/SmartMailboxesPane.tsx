/**
 * Unified mailbox settings view with drill-in editors for smart and source mailboxes.
 *
 */
import type { AccountRow } from '@/gen'
import { Plus } from 'lucide-react'
import type {
  AppSettings,
  SmartMailbox,
  SmartMailboxSummary,
} from '../../../data/transport/api/index'
import { renderSmartMailboxIcon, smartMailboxAccent } from '../../../domain/role'
import { Button } from '../../ui/form/button'
import { SortableList, SortableRow } from '../../ui/display/SortableList'
import { useSidebarReorder } from '../../sidebar/hooks/useSidebarReorder'
import {
  FeedbackBanner,
  SettingsBackButton,
  SettingsList,
  SettingsPage,
  SettingsPageHeader,
} from '../panel/shared'
import type { MailboxEditorTarget } from '../panel/types'
import {
  SmartMailboxDetail,
  SourceMailboxDetail,
} from './pane/Details'
import { SmartMailboxesEmptyState } from './pane/EmptyState'
import { MailboxListRow } from './pane/MailboxListRow'
import { SourceMailboxListSection } from './pane/SourceMailboxListSection'

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
  onSaved,
  onAutomationSettingsSaved,
  onDeleted,
}: {
  smartMailboxes: SmartMailboxSummary[]
  accounts: AccountRow[]
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
  onSaved: (mailbox: SmartMailbox) => Promise<void>
  onAutomationSettingsSaved: (settings: AppSettings) => Promise<void>
  onDeleted: (mailboxId: string) => Promise<void>
}) {
  const { reorderSmartMailboxes } = useSidebarReorder()
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
              editingSmartMailbox={editingSmartMailbox}
              editorKey={editorKey}
              accounts={accounts}
              settings={settings}
              onCreateMailbox={onCreateMailbox}
              onSaved={onSaved}
              onAutomationSettingsSaved={onAutomationSettingsSaved}
              onDeleted={onDeleted}
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
            <SortableList
              ids={smartMailboxes.map((mailbox) => mailbox.id)}
              onReorder={reorderSmartMailboxes}
            >
              {smartMailboxes.map((mailbox) => (
                <SortableRow key={mailbox.id} id={mailbox.id}>
                  <MailboxListRow
                    accent={smartMailboxAccent(mailbox.role, mailbox.name)}
                    icon={renderSmartMailboxIcon(
                      mailbox.role,
                      mailbox.defaultKey,
                      15,
                    )}
                    label={mailbox.name}
                    sublabel={`${mailbox.totalMessages} messages · ${mailbox.unreadMessages} unread`}
                    badge={mailbox.kind === 'default' ? 'default' : null}
                    onClick={() => onSelectSmartMailbox(mailbox.id)}
                  />
                </SortableRow>
              ))}
            </SortableList>
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
