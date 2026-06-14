/**
 * Accounts view: centered list with drill-in account editing.
 *
 * @spec docs/L1-api#account-crud-lifecycle
 */
import type { UseMutationResult } from '@tanstack/react-query'
import { useState } from 'react'

import type { AccountOverview } from '../../api/types'
import { AccountEditor } from './AccountEditor'
import { AccountList, AccountsEmptyState } from './accounts-pane/AccountList'
import { AccountSetupChoice } from './accounts-pane/AccountSetupChoice'
import {
  SettingsBackButton,
  SettingsPage,
  SettingsPageHeader,
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
    action: 'enable' | 'disable' | 'delete' | 'sync' | 'repairMetadata',
    account: AccountOverview,
  ) => void
  onSaved: (account: AccountOverview) => Promise<void>
  onVerified: () => Promise<void>
  commandMutation: UseMutationResult<
    unknown,
    Error,
    {
      action: 'enable' | 'disable' | 'delete' | 'sync' | 'repairMetadata'
      account: AccountOverview
    },
    unknown
  >
  commandError: string | null
}) {
  const [isManualCreate, setIsManualCreate] = useState(false)
  const handleBackToAccounts = () => {
    setIsManualCreate(false)
    onBackToAccounts()
  }

  if (selectedAccountId !== null) {
    return (
      <section className="ph-scroll h-full min-h-0 overflow-y-auto px-6 py-8">
        <SettingsPage>
          <SettingsBackButton
            ariaLabel="Back to accounts"
            onClick={handleBackToAccounts}
          >
            Accounts
          </SettingsBackButton>

          {selectedAccountId === 'new' && !isManualCreate ? (
            <AccountSetupChoice onManual={() => setIsManualCreate(true)} />
          ) : selectedAccountId === 'new' || editingAccount ? (
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
          description="Connect each mail source Posthaste should sync. Accounts keep their own credentials, status, and sync controls."
        />

        <AccountList
          accounts={accounts}
          onCreateAccount={onCreateAccount}
          onSelectAccount={onSelectAccount}
        />
      </SettingsPage>
    </section>
  )
}
