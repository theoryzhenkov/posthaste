import type { UseMutationResult } from '@tanstack/react-query'

import type {
  AppSettings,
  CachePolicy,
  Notifications,
  SmartMailbox,
  SmartMailboxSummary,
} from '../../api/types'
import type {
  AccountRow,
  AccountSettingsResult,
  CommandAccepted,
} from '@/gen'
import { DEFAULT_UNDO_SEND_DELAY_SECONDS } from '../../api/types/settings'
import type { SettingsSurfaceDescriptor } from '../../surfaces'
import type {
  AccountActionTarget,
  AccountCommandAction,
} from './account-editor/AccountActions'
import { AccountsPane } from './AccountsPane'
import { AppearancePane } from './AppearancePane'
import { AutomationsPane } from './AutomationsPane'
import { GeneralPane } from './GeneralPane'
import { NotificationsPane } from './NotificationsPane'
import { OutboxPane } from './OutboxPane'
import { StoragePane } from './StoragePane'
import { TagsPane } from './TagsPane'
import { TroubleshootingPane } from './TroubleshootingPane'
import {
  SmartMailboxesPane,
  type MailboxEditorTarget,
} from './SmartMailboxesPane'
import type { EditorTarget } from './types'

interface SettingsPanelContentProps {
  accounts: AccountRow[]
  accountCommandError: string | null
  activeCategory: SettingsSurfaceDescriptor['params']['category']
  commandMutation: UseMutationResult<
    CommandAccepted,
    Error,
    { action: AccountCommandAction; account: AccountActionTarget },
    void
  >
  cacheMutation: UseMutationResult<CommandAccepted, Error, CachePolicy>
  notificationsMutation: UseMutationResult<CommandAccepted, Error, Notifications>
  defaultAccountId: string | null | undefined
  defaultMutation: UseMutationResult<CommandAccepted, Error, string | null>
  editingAccount: AccountSettingsResult | null
  editingSmartMailbox: SmartMailbox | SmartMailboxSummary | null
  editorKey: string
  effectiveEditorTarget: EditorTarget | null
  effectiveSmartMailboxTarget: string | null
  mailboxEditorTarget: MailboxEditorTarget | null
  settings: AppSettings | null
  smartMailboxActionError: string | null
  smartMailboxActionPendingKey: string | null
  smartMailboxEditorKey: string
  smartMailboxSummaries: SmartMailboxSummary[]
  onAutomationSettingsSaved: (settings: AppSettings) => Promise<void>
  onBackToAccounts: () => void
  onBackToMailboxes: () => void
  onCreateAccount: () => void
  onCreateMailbox: () => void
  onDefaultAccountChange: (accountId: string | null) => void
  onUndoSendDelayChange: (seconds: number) => void
  isComposePending: boolean
  onDeletedSmartMailbox: (mailboxId: string) => Promise<void>
  onResetSmartMailboxes: () => void
  onSavedAccount: (accountId: string) => Promise<void>
  onSavedSmartMailbox: (mailbox: SmartMailbox) => Promise<void>
  onSelectAccount: (accountId: string) => void
  onSelectSmartMailbox: (mailboxId: string) => void
  onSelectSourceMailbox: (accountId: string, mailboxId: string) => void
  onVerifiedAccount: () => Promise<void>
}

export function SettingsPanelContent({
  accounts,
  accountCommandError,
  activeCategory,
  cacheMutation,
  notificationsMutation,
  commandMutation,
  defaultAccountId,
  defaultMutation,
  editingAccount,
  editingSmartMailbox,
  editorKey,
  effectiveEditorTarget,
  effectiveSmartMailboxTarget,
  mailboxEditorTarget,
  settings,
  smartMailboxActionError,
  smartMailboxActionPendingKey,
  smartMailboxEditorKey,
  smartMailboxSummaries,
  onAutomationSettingsSaved,
  onBackToAccounts,
  onBackToMailboxes,
  onCreateAccount,
  onCreateMailbox,
  onDefaultAccountChange,
  onUndoSendDelayChange,
  isComposePending,
  onDeletedSmartMailbox,
  onResetSmartMailboxes,
  onSavedAccount,
  onSavedSmartMailbox,
  onSelectAccount,
  onSelectSmartMailbox,
  onSelectSourceMailbox,
  onVerifiedAccount,
}: SettingsPanelContentProps) {
  return (
    <main className="min-w-0 flex-1 bg-background">
      <div className="h-full min-h-0 overflow-hidden bg-transparent">
        {activeCategory === 'general' && (
          <div className="ph-scroll h-full min-h-0 overflow-y-auto px-6 py-8">
            <GeneralPane
              accounts={accounts}
              defaultAccountId={defaultAccountId}
              onDefaultAccountChange={onDefaultAccountChange}
              isPending={defaultMutation.isPending}
              undoSendDelaySeconds={
                settings?.compose?.undoSendDelaySeconds ??
                DEFAULT_UNDO_SEND_DELAY_SECONDS
              }
              onUndoSendDelayChange={onUndoSendDelayChange}
              isComposePending={isComposePending}
            />
          </div>
        )}

        {activeCategory === 'appearance' && (
          <div className="ph-scroll h-full min-h-0 overflow-y-auto px-6 py-8">
            <AppearancePane />
          </div>
        )}

        {activeCategory === 'storage' && (
          <div className="ph-scroll h-full min-h-0 overflow-y-auto px-6 py-8">
            <StoragePane
              cachePolicy={settings?.cachePolicy}
              onChange={(policy) => cacheMutation.mutate(policy)}
              isPending={cacheMutation.isPending}
            />
          </div>
        )}

        {activeCategory === 'notifications' && (
          <div className="ph-scroll h-full min-h-0 overflow-y-auto px-6 py-8">
            <NotificationsPane
              notifications={settings?.notifications}
              onChange={(notifications) =>
                notificationsMutation.mutate(notifications)
              }
              isPending={notificationsMutation.isPending}
            />
          </div>
        )}

        {activeCategory === 'outbox' && (
          <div className="ph-scroll h-full min-h-0 overflow-y-auto px-6 py-8">
            <OutboxPane />
          </div>
        )}

        {activeCategory === 'accounts' && (
          <AccountsPane
            accounts={accounts}
            selectedAccountId={effectiveEditorTarget}
            editingAccount={editingAccount}
            editorKey={editorKey}
            onSelectAccount={onSelectAccount}
            onBackToAccounts={onBackToAccounts}
            onCreateAccount={onCreateAccount}
            onCommand={(action, account) =>
              commandMutation.mutate({ action, account })
            }
            onSaved={onSavedAccount}
            onVerified={onVerifiedAccount}
            commandMutation={commandMutation}
            commandError={accountCommandError}
          />
        )}

        {activeCategory === 'mailboxes' && (
          <SmartMailboxesPane
            smartMailboxes={smartMailboxSummaries}
            accounts={accounts}
            settings={settings}
            selectedMailboxTarget={
              mailboxEditorTarget?.kind === 'smart'
                ? effectiveSmartMailboxTarget
                  ? { kind: 'smart', id: effectiveSmartMailboxTarget }
                  : null
                : mailboxEditorTarget
            }
            editingSmartMailbox={editingSmartMailbox}
            editorKey={smartMailboxEditorKey}
            actionPendingKey={smartMailboxActionPendingKey}
            actionError={smartMailboxActionError}
            onSelectSmartMailbox={onSelectSmartMailbox}
            onSelectSourceMailbox={onSelectSourceMailbox}
            onBackToMailboxes={onBackToMailboxes}
            onCreateMailbox={onCreateMailbox}
            onResetDefaults={onResetSmartMailboxes}
            onSaved={onSavedSmartMailbox}
            onAutomationSettingsSaved={onAutomationSettingsSaved}
            onDeleted={onDeletedSmartMailbox}
          />
        )}

        {activeCategory === 'automations' && (
          <div className="ph-scroll h-full min-h-0 overflow-y-auto px-6 py-8">
            <AutomationsPane />
          </div>
        )}

        {activeCategory === 'tags' && (
          <div className="ph-scroll h-full min-h-0 overflow-y-auto px-6 py-8">
            <TagsPane settings={settings} />
          </div>
        )}

        {activeCategory === 'troubleshooting' && (
          <div className="ph-scroll h-full min-h-0 overflow-y-auto px-6 py-8">
            <TroubleshootingPane />
          </div>
        )}
      </div>
    </main>
  )
}
