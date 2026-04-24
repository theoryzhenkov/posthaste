/**
 * Accounts view: centered list with drill-in account editing.
 *
 * @spec docs/L1-api#account-crud-lifecycle
 */
import type { UseMutationResult } from '@tanstack/react-query'
import { ArrowLeft, Plus, UserPlus } from 'lucide-react'
import type { AccountOverview } from '../../api/types'
import { brandAccents } from '../../design/tokens'
import { AccountEditor } from './AccountEditor'
import { Button } from '../ui/button'
import { StatusDot } from './shared'
import type { EditorTarget } from './types'

const ACCOUNT_ACCENTS = [
  brandAccents.blue,
  brandAccents.coral,
  brandAccents.sage,
  brandAccents.violet,
  brandAccents.amber,
] as const

function accountAccent(account: AccountOverview): string {
  const seed = `${account.id}:${account.name}`
  let hash = 0
  for (let index = 0; index < seed.length; index += 1) {
    hash = (hash * 31 + seed.charCodeAt(index)) >>> 0
  }
  return ACCOUNT_ACCENTS[hash % ACCOUNT_ACCENTS.length]
}

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
}) {
  if (selectedAccountId !== null) {
    return (
      <section className="ph-scroll h-full min-h-0 overflow-y-auto px-6 py-8">
        <div className="mx-auto max-w-[760px]">
          <Button
            aria-label="Back to accounts"
            size="sm"
            variant="ghost"
            type="button"
            onClick={onBackToAccounts}
            className="mb-3 h-7 rounded-md px-2 text-[12px] text-muted-foreground hover:bg-[var(--list-hover)] hover:text-foreground"
          >
            <ArrowLeft size={14} strokeWidth={1.5} />
            Accounts
          </Button>

          {selectedAccountId === 'new' || editingAccount ? (
            <AccountEditor
              key={editorKey}
              editorTarget={selectedAccountId}
              editingAccount={editingAccount}
              onSaved={onSaved}
              onVerified={onVerified}
              onCommand={onCommand}
              isCommandPending={commandMutation.isPending}
            />
          ) : (
            <AccountsEmptyState onCreateAccount={onCreateAccount} />
          )}
        </div>
      </section>
    )
  }

  return (
    <section className="ph-scroll h-full min-h-0 overflow-y-auto px-6 py-8">
      <div className="mx-auto flex max-w-[760px] flex-col">
        <header>
          <h1 className="text-[24px] font-semibold leading-tight text-foreground">
            Connected accounts
          </h1>
          <p className="mt-2 max-w-[620px] text-[13px] leading-6 text-muted-foreground">
            Connect each mail source PostHaste should sync. Accounts keep their
            own credentials, status, and sync controls.
          </p>
        </header>

        {accounts.length === 0 ? (
          <div className="mt-10">
            <AccountsEmptyState onCreateAccount={onCreateAccount} />
          </div>
        ) : (
          <div className="mt-7 overflow-hidden rounded-lg border border-border-soft bg-bg-elev/45">
            <div className="flex min-h-[48px] items-center justify-between gap-3 border-b border-border-soft px-4">
              <h2 className="text-[13px] font-semibold text-foreground">
                {accounts.length} connected{' '}
                {accounts.length === 1 ? 'account' : 'accounts'}
              </h2>
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
            </div>
            {accounts.map((account) => (
              <AccountListRow
                key={account.id}
                accent={accountAccent(account)}
                label={account.name}
                sublabel={
                  account.transport.username ?? account.driver.toUpperCase()
                }
                isDefault={account.isDefault}
                leading={<StatusDot status={account.status} />}
                onClick={() => onSelectAccount(account.id)}
              />
            ))}
          </div>
        )}
      </div>
    </section>
  )
}

function AccountListRow({
  accent,
  label,
  sublabel,
  isDefault,
  leading,
  onClick,
}: {
  accent: string
  label: string
  sublabel?: string
  isDefault?: boolean
  leading: React.ReactNode
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
        {leading}
      </span>
      <span className="min-w-0 flex-1">
        <span className="flex items-center gap-1.5">
          <span className="truncate text-[13px] font-medium text-foreground">
            {label}
          </span>
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
    <div className="flex min-h-[220px] flex-col items-center justify-center rounded-lg border border-dashed border-border-soft bg-bg-elev/45 px-6 text-center">
      <UserPlus
        size={36}
        strokeWidth={1.5}
        className="text-muted-foreground/40"
      />
      <div className="mt-4">
        <p className="text-[13px] font-medium">No accounts yet</p>
        <p className="mt-1 text-[13px] text-muted-foreground">
          Add one to start syncing your mail.
        </p>
      </div>
      <Button
        size="sm"
        variant="outline"
        type="button"
        onClick={onCreateAccount}
        className="mt-4 rounded-md border-border bg-bg-elev"
      >
        <Plus size={13} strokeWidth={2} />
        New account
      </Button>
    </div>
  )
}
