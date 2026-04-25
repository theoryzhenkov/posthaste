/**
 * Accounts view: centered list with drill-in account editing.
 *
 * @spec docs/L1-api#account-crud-lifecycle
 */
import type { UseMutationResult } from '@tanstack/react-query'
import { Plus, UserPlus } from 'lucide-react'
import type { AccountOverview } from '../../api/types'
import { AccountMark } from '../AccountMark'
import { AccountEditor } from './AccountEditor'
import { Button } from '../ui/button'
import {
  SettingsBackButton,
  SettingsEmptyState,
  SettingsList,
  SettingsPage,
  SettingsPageHeader,
  StatusDot,
} from './shared'
import type { EditorTarget } from './types'

export function AccountsPane({
  accounts,
  selectedAccountId,
  editingAccount,
  editorKey,
  onSelectAccount,
  onBackToAccounts,
  onCreateAccount,
  onCommand,
  onSaved,
  onVerified,
  commandMutation,
  commandError,
}: {
  accounts: AccountOverview[]
  selectedAccountId: EditorTarget | null
  editingAccount: AccountOverview | null
  editorKey: string
  onSelectAccount: (accountId: string) => void
  onBackToAccounts: () => void
  onCreateAccount: () => void
  onCommand: (
    action: 'enable' | 'disable' | 'delete' | 'sync',
    account: AccountOverview,
  ) => void
  onSaved: (account: AccountOverview) => Promise<void>
  onVerified: () => Promise<void>
  commandMutation: UseMutationResult<
    unknown,
    Error,
    {
      action: 'enable' | 'disable' | 'delete' | 'sync'
      account: AccountOverview
    },
    unknown
  >
  commandError: string | null
}) {
  if (selectedAccountId !== null) {
    return (
      <section className="ph-scroll h-full min-h-0 overflow-y-auto px-6 py-8">
        <SettingsPage>
          <SettingsBackButton
            ariaLabel="Back to accounts"
            onClick={onBackToAccounts}
          >
            Accounts
          </SettingsBackButton>

          {selectedAccountId === 'new' || editingAccount ? (
            <AccountEditor
              key={editorKey}
              editorTarget={selectedAccountId}
              editingAccount={editingAccount}
              onSaved={onSaved}
              onVerified={onVerified}
              onCommand={onCommand}
              isCommandPending={commandMutation.isPending}
              commandError={commandError}
            />
          ) : (
            <AccountsEmptyState onCreateAccount={onCreateAccount} />
          )}
        </SettingsPage>
      </section>
    )
  }

  return (
    <section className="ph-scroll h-full min-h-0 overflow-y-auto px-6 py-8">
      <SettingsPage>
        <SettingsPageHeader
          title="Connected accounts"
          description="Connect each mail source PostHaste should sync. Accounts keep their own credentials, status, and sync controls."
        />

        {accounts.length === 0 ? (
          <div className="mt-10">
            <AccountsEmptyState onCreateAccount={onCreateAccount} />
          </div>
        ) : (
          <SettingsList
            title={`${accounts.length} connected ${
              accounts.length === 1 ? 'account' : 'accounts'
            }`}
            actions={
              <Button
                aria-label="New account"
                size="icon-sm"
                variant="outline"
                type="button"
                onClick={onCreateAccount}
                className="size-7 rounded-[5px] border-border bg-background text-muted-foreground hover:text-foreground"
              >
                <Plus size={14} strokeWidth={1.8} />
              </Button>
            }
          >
            {accounts.map((account) => (
              <AccountListRow
                key={account.id}
                account={account}
                label={account.name}
                sublabel={
                  account.emailPatterns?.[0] ??
                  account.transport.username ??
                  account.fullName ??
                  undefined
                }
                isDefault={account.isDefault}
                onClick={() => onSelectAccount(account.id)}
              />
            ))}
          </SettingsList>
        )}
      </SettingsPage>
    </section>
  )
}

function AccountListRow({
  account,
  label,
  sublabel,
  isDefault,
  onClick,
}: {
  account: AccountOverview
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
      <AccountMark appearance={account.appearance} />
      <span className="min-w-0 flex-1">
        <span className="flex items-center gap-1.5">
          <span className="truncate text-[13px] font-medium text-foreground">
            {label}
          </span>
          <StatusDot status={account.status} className="size-1.5" />
          {isDefault && (
            <span
              className="shrink-0 rounded-sm bg-background/80 px-1.5 py-0.5 font-mono text-[9px] uppercase tracking-[0.18em] text-muted-foreground"
              title="Default account"
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

function AccountsEmptyState({
  onCreateAccount,
}: {
  onCreateAccount: () => void
}) {
  return (
    <SettingsEmptyState
      icon={<UserPlus size={36} strokeWidth={1.5} />}
      title="No accounts yet"
      description="Add one to start syncing your mail."
      action={
        <Button
          size="sm"
          variant="outline"
          type="button"
          onClick={onCreateAccount}
          className="rounded-md border-border bg-bg-elev"
        >
          <Plus size={13} strokeWidth={2} />
          New account
        </Button>
      }
    />
  )
}
