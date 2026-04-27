/**
 * Accounts view: centered list with drill-in account editing.
 *
 * @spec docs/L1-api#account-crud-lifecycle
 */
import { useMutation, type UseMutationResult } from '@tanstack/react-query'
import { Cloud, Mail, Plus, Settings2, UserPlus } from 'lucide-react'
import { useState } from 'react'
import { buildOAuthRedirectUri, startProviderOAuth } from '../../api/client'
import type { AccountOverview, ProviderHint } from '../../api/types'
import { providerOAuthClientCredentials } from '../../config/oauthProviders'
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
                  account.connection.username ??
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

function AccountSetupChoice({ onManual }: { onManual: () => void }) {
  const [errorMessage, setErrorMessage] = useState<string | null>(null)
  const [startedProvider, setStartedProvider] = useState<ProviderHint | null>(
    null,
  )
  const startOAuthMutation = useMutation({
    mutationFn: async (provider: ProviderHint) => {
      const credentials = providerOAuthClientCredentials[provider]
      const clientId = credentials?.clientId.trim()
      if (!clientId) {
        throw new Error(
          `${providerLabel(provider)} OAuth client ID is not configured`,
        )
      }
      return startProviderOAuth({
        provider,
        clientId,
        clientSecret: credentials?.clientSecret,
        redirectUri: buildOAuthRedirectUri(),
      })
    },
    onSuccess: (session, provider) => {
      setErrorMessage(null)
      setStartedProvider(provider)
      window.open(session.authorizationUrl, '_blank', 'noopener,noreferrer')
    },
    onError: (error: Error) => {
      setStartedProvider(null)
      setErrorMessage(error.message)
    },
  })

  return (
    <div className="pb-8">
      <SettingsPageHeader
        title="New account"
        description="Choose a provider, or configure the connection manually."
      />

      <div className="grid gap-3 sm:grid-cols-2">
        <ProviderButton
          icon={<Mail size={17} strokeWidth={1.8} />}
          label="Google"
          disabled={startOAuthMutation.isPending}
          onClick={() => startOAuthMutation.mutate('gmail')}
        />
        <ProviderButton
          icon={<Cloud size={17} strokeWidth={1.8} />}
          label="Outlook"
          disabled={startOAuthMutation.isPending}
          onClick={() => startOAuthMutation.mutate('outlook')}
        />
        <ProviderButton
          icon={<Settings2 size={17} strokeWidth={1.8} />}
          label="Manual"
          disabled={startOAuthMutation.isPending}
          onClick={onManual}
        />
      </div>

      {startedProvider && (
        <p className="mt-4 text-[12px] text-muted-foreground">
          {providerLabel(startedProvider)} authorization opened in your browser.
        </p>
      )}
      {errorMessage && (
        <p className="mt-4 text-[12px] text-destructive">{errorMessage}</p>
      )}
    </div>
  )
}

function ProviderButton({
  icon,
  label,
  disabled,
  onClick,
}: {
  icon: React.ReactNode
  label: string
  disabled: boolean
  onClick: () => void
}) {
  return (
    <Button
      type="button"
      variant="outline"
      disabled={disabled}
      onClick={onClick}
      className="h-12 justify-start rounded-md border-border bg-bg-elev px-4 text-[13px]"
    >
      <span className="flex size-7 items-center justify-center rounded-md bg-background text-muted-foreground">
        {icon}
      </span>
      {label}
    </Button>
  )
}

function providerLabel(provider: ProviderHint): string {
  switch (provider) {
    case 'gmail':
      return 'Google'
    case 'outlook':
      return 'Outlook'
    case 'icloud':
      return 'iCloud'
    case 'generic':
      return 'Provider'
  }
}
