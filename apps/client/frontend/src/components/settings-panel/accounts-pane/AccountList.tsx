import { Plus, UserPlus } from 'lucide-react'

import { useAccountSettings } from '@/data'
import type { AccountRow } from '@/gen'
import { AccountMark } from '../../AccountMark'
import { Button } from '../../ui/button'
import { SortableList, SortableRow } from '../../ui/SortableList'
import { useSidebarReorder } from '../../sidebar/useSidebarReorder'
import { AccountHealthNotice } from '../AccountHealthNotice'
import { SyncProgressMeter } from '../SyncProgressMeter'
import { SettingsEmptyState, SettingsList, StatusDot } from '../shared'

export function AccountList({
  accounts,
  onCreateAccount,
  onSelectAccount,
  onRetryAccount,
  isRetryPending = false,
}: {
  accounts: AccountRow[]
  onCreateAccount: () => void
  onSelectAccount: (accountId: string) => void
  /** Trigger a fresh sync/connection attempt for an errored account. */
  onRetryAccount?: (account: AccountRow) => void
  isRetryPending?: boolean
}) {
  const { reorderAccounts } = useSidebarReorder()
  if (accounts.length === 0) {
    return (
      <div className="mt-10">
        <AccountsEmptyState onCreateAccount={onCreateAccount} />
      </div>
    )
  }

  return (
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
      <SortableList
        ids={accounts.map((account) => account.id)}
        onReorder={reorderAccounts}
      >
        {accounts.map((account) => (
          <SortableRow key={account.id} id={account.id}>
            <AccountListRow
              account={account}
              onClick={() => onSelectAccount(account.id)}
            />
            <div className="px-4 pb-2 empty:hidden">
              <AccountHealthNotice
                account={account}
                onAction={onRetryAccount}
                isActionPending={isRetryPending}
                compact
              />
            </div>
          </SortableRow>
        ))}
      </SortableList>
    </SettingsList>
  )
}

function AccountListRow({
  account,
  onClick,
}: {
  account: AccountRow
  onClick: () => void
}) {
  // Appearance and addresses live on the per-account settings answer; the
  // row falls back to a letter mark while it loads.
  const settingsQuery = useAccountSettings(account.id)
  const appearance = settingsQuery.data?.appearance ?? {
    kind: 'initials' as const,
    initials: account.name.trim().charAt(0).toUpperCase() || 'A',
    colorHue: 0,
  }
  const sublabel =
    settingsQuery.data?.emailPatterns?.[0] ??
    settingsQuery.data?.transport.username ??
    account.fullName ??
    undefined

  return (
    <button
      type="button"
      onClick={onClick}
      className="group flex min-h-[56px] w-full items-center gap-3 border-b border-border-soft px-4 text-left transition-colors last:border-b-0 hover:bg-[var(--list-hover)]"
    >
      <AccountMark appearance={appearance} />
      <span className="min-w-0 flex-1">
        <span className="flex items-center gap-1.5">
          <span className="truncate text-[13px] font-medium text-foreground">
            {account.name}
          </span>
          <StatusDot status={account.status} className="size-1.5" />
          {account.isDefault && (
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
        <SyncProgressMeter account={account} compact />
      </span>
      <span className="text-[12px] text-muted-foreground group-hover:text-foreground">
        Edit
      </span>
    </button>
  )
}

export function AccountsEmptyState({
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
