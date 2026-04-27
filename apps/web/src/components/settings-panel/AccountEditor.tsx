/**
 * Account create/edit form with save, verify, and secret management.
 *
 * @spec docs/L1-api#account-crud-lifecycle
 * @spec docs/L1-api#secret-management
 */
import { useMutation } from '@tanstack/react-query'
import { useEffect, useMemo, useRef, useState } from 'react'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from '../ui/alert-dialog'
import { Button } from '../ui/button'
import { Input } from '../ui/input'
import { createAccount, updateAccount, verifyAccount } from '../../api/client'
import type {
  AccountOverview,
  ProviderAuthKind,
  ProviderHint,
  VerificationResponse,
} from '../../api/types'
import { AccountMark } from '../AccountMark'
import {
  buildAccountEditorModel,
  type AccountEditorConnectionModel,
  type ExistingAccountEditorModel,
  type ManagedOAuthConnectionModel,
} from './accountEditorModel'
import {
  buildAccountAppearanceInput,
  buildCreateAccountPayload,
  buildUpdateAccountPayload,
  emptyAccountForm,
  formFromAccount,
  normalizeAccountInitials,
} from './helpers'
import { SyncProgressMeter } from './SyncProgressMeter'
import { FeedbackBanner, Field, StatusDot } from './shared'
import { SettingsFooter, SettingsPageHeader, SettingsSection } from './shared'
import type { EditorTarget } from './types'
import type { AccountFormState } from './types'

const accountHueGradient =
  'linear-gradient(90deg, oklch(0.68 0.17 0), oklch(0.68 0.17 45), oklch(0.68 0.17 90), oklch(0.68 0.17 145), oklch(0.68 0.17 205), oklch(0.68 0.17 260), oklch(0.68 0.17 315), oklch(0.68 0.17 360))'

function accountAppearanceSignature(
  appearance: AccountOverview['appearance'],
): string {
  const imagePart = appearance.kind === 'image' ? appearance.imageId : ''
  return `${appearance.kind}:${appearance.initials}:${appearance.colorHue}:${imagePart}`
}

function appearanceFromForm(
  form: AccountFormState,
): AccountOverview['appearance'] {
  return {
    kind: 'initials',
    initials: normalizeAccountInitials(form.appearanceInitials || form.name),
    colorHue: Math.min(360, Math.max(0, Math.round(form.appearanceColorHue))),
  }
}

function accountFieldsSignature(
  form: AccountFormState,
  connection: AccountEditorConnectionModel,
): string {
  const signature = {
    name: form.name.trim(),
    fullName: form.fullName.trim(),
    emailPatternsText: form.emailPatternsText.trim(),
  }
  if (connection.kind === 'managedOAuth') {
    return JSON.stringify(signature)
  }
  return JSON.stringify({
    ...signature,
    baseUrl: form.baseUrl.trim(),
    username: form.username.trim(),
    passwordChanged: form.password.trim().length > 0,
  })
}

/**
 * Account editor form: create new or edit existing accounts.
 *
 * Hides backend-only account IDs and secret write modes from users while
 * preserving post-save JMAP verification and account-level actions.
 *
 * @spec docs/L1-api#account-crud-lifecycle
 * @spec docs/L1-api#secret-management
 */
export function AccountEditor({
  editorTarget,
  editingAccount,
  onSaved,
  onVerified,
  onCommand,
  isCommandPending,
  commandError,
}: {
  editorTarget: EditorTarget
  editingAccount: AccountOverview | null
  onSaved: (account: AccountOverview) => Promise<void>
  onVerified: () => Promise<void>
  onCommand: (
    action: 'enable' | 'disable' | 'delete' | 'sync',
    account: AccountOverview,
  ) => void
  isCommandPending: boolean
  commandError: string | null
}) {
  const editorModel = useMemo(
    () => buildAccountEditorModel(editorTarget, editingAccount),
    [editorTarget, editingAccount],
  )
  const [form, setForm] = useState(() =>
    editingAccount ? formFromAccount(editingAccount) : emptyAccountForm(),
  )
  const [errorMessage, setErrorMessage] = useState<string | null>(null)
  const [verification, setVerification] = useState<VerificationResponse | null>(
    null,
  )
  const [savedAccountFieldsSignature, setSavedAccountFieldsSignature] =
    useState(() => accountFieldsSignature(form, editorModel.connection))

  const saveMutation = useMutation({
    mutationFn: async (currentForm: typeof form) => {
      return editorModel.kind === 'new'
        ? createAccount(buildCreateAccountPayload(currentForm))
        : updateAccount(
            editorModel.account.id,
            buildUpdateAccountPayload(currentForm, editorModel),
          )
    },
    onSuccess: async (account) => {
      setErrorMessage(null)
      setVerification(null)
      const savedForm = formFromAccount(account)
      const savedEditorModel = buildAccountEditorModel(account.id, account)
      setSavedAccountFieldsSignature(
        accountFieldsSignature(savedForm, savedEditorModel.connection),
      )
      setForm(savedForm)
      await onSaved(account)
    },
    onError: (error: Error) => {
      setErrorMessage(error.message)
    },
  })

  const verifyMutation = useMutation({
    mutationFn: (accountId: string) => verifyAccount(accountId),
    onSuccess: async (result) => {
      setVerification(result)
      setErrorMessage(null)
      await onVerified()
    },
    onError: (error: Error) => {
      setVerification(null)
      setErrorMessage(error.message)
    },
  })

  const existingModel: ExistingAccountEditorModel | null =
    editorModel.kind === 'new' ? null : editorModel
  const existingAccount = existingModel?.account ?? null
  const formAppearance = appearanceFromForm(form)
  const hasUnsavedAccountChanges =
    accountFieldsSignature(form, editorModel.connection) !==
    savedAccountFieldsSignature

  return (
    <div className="pb-8">
      <SettingsPageHeader
        title={
          editorTarget === 'new'
            ? 'New account'
            : (existingAccount?.name ?? 'Account')
        }
        leading={
          <AccountMark
            appearance={formAppearance}
            className="size-10 rounded-md text-[14px]"
          />
        }
        meta={
          <p className="flex items-center gap-1.5 text-[12px] text-muted-foreground">
            {existingModel ? (
              <AccountHeaderMeta model={existingModel} />
            ) : (
              'Configure the account, then apply it.'
            )}
          </p>
        }
        actions={
          existingAccount ? (
            <AccountActions
              account={existingAccount}
              onCommand={onCommand}
              isCommandPending={isCommandPending}
            />
          ) : null
        }
      />

      {existingAccount?.syncProgress && (
        <div className="-mt-4 mb-4">
          <SyncProgressMeter account={existingAccount} />
        </div>
      )}

      {existingAccount?.lastSyncError && (
        <div className="mt-4">
          <FeedbackBanner tone="error">
            {existingAccount.lastSyncError}
          </FeedbackBanner>
        </div>
      )}

      <SettingsSection title="Identity">
        <div className="grid gap-3 sm:grid-cols-2">
          <Field
            label="Account name"
            value={form.name}
            onChange={(value) =>
              setForm((current) => ({ ...current, name: value }))
            }
          />
          <Field
            label="Full name"
            value={form.fullName}
            placeholder="Ada Lovelace"
            onChange={(value) =>
              setForm((current) => ({ ...current, fullName: value }))
            }
          />
        </div>

        <label className="grid gap-1.5 text-[13px]">
          <span className="text-[12px] font-medium text-muted-foreground">
            Email addresses
          </span>
          <textarea
            className="min-h-[72px] w-full resize-y rounded-md border border-border bg-background px-2.5 py-2 text-[13px] shadow-none outline-none placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50"
            value={form.emailPatternsText}
            placeholder={'you@example.com\n*@example.com'}
            onChange={(event) =>
              setForm((current) => ({
                ...current,
                emailPatternsText: event.target.value,
              }))
            }
          />
        </label>
      </SettingsSection>

      <SettingsSection title="Appearance">
        <AccountAppearanceFields
          accountId={existingAccount?.id ?? null}
          form={form}
          onChange={setForm}
          onSaved={onSaved}
        />
      </SettingsSection>

      <ConnectionEditor
        connection={editorModel.connection}
        form={form}
        onChange={setForm}
      />

      <SettingsFooter>
        {verification?.identityEmail && (
          <FeedbackBanner tone="success">
            Verified identity: {verification.identityEmail}
          </FeedbackBanner>
        )}
        {errorMessage && (
          <FeedbackBanner tone="error">{errorMessage}</FeedbackBanner>
        )}
        {commandError && (
          <FeedbackBanner tone="error">{commandError}</FeedbackBanner>
        )}

        <div className="flex flex-wrap items-center gap-2">
          <Button
            type="button"
            onClick={() => saveMutation.mutate(form)}
            disabled={saveMutation.isPending || !hasUnsavedAccountChanges}
            className="bg-brand-coral text-white hover:bg-brand-coral/90"
          >
            Apply
          </Button>
          {existingAccount && (
            <Button
              type="button"
              variant="outline"
              onClick={() => verifyMutation.mutate(existingAccount.id)}
              disabled={
                verifyMutation.isPending ||
                saveMutation.isPending ||
                hasUnsavedAccountChanges
              }
              className="rounded-md border-border bg-background"
            >
              Verify connection
            </Button>
          )}
          <span className="text-[12px] text-muted-foreground">
            {hasUnsavedAccountChanges ? 'Unsaved changes' : 'Saved'}
          </span>
        </div>
      </SettingsFooter>

      {existingAccount && (
        <DangerSection
          account={existingAccount}
          onCommand={onCommand}
          isCommandPending={isCommandPending}
        />
      )}
    </div>
  )
}

function AccountHeaderMeta({ model }: { model: ExistingAccountEditorModel }) {
  return (
    <>
      <StatusDot status={model.account.status} className="size-1.5" />
      <span className="font-mono uppercase tracking-[0.12em]">
        {model.account.status}
      </span>
      <span aria-hidden>·</span>
      <span>{providerLabel(model.account.connection.provider)}</span>
      <span aria-hidden>·</span>
      <span>{authLabel(model.account.connection.auth)}</span>
    </>
  )
}

function ConnectionEditor({
  connection,
  form,
  onChange,
}: {
  connection: AccountEditorConnectionModel
  form: AccountFormState
  onChange: React.Dispatch<React.SetStateAction<AccountFormState>>
}) {
  switch (connection.kind) {
    case 'managedOAuth':
      return <OAuthConnectionDetails connection={connection} />
    case 'manualCredentials':
      return (
        <>
          <SettingsSection title="Server">
            <div className="grid gap-3 sm:grid-cols-2">
              <Field
                label="Base URL"
                value={form.baseUrl}
                placeholder="https://mail.example.com/jmap"
                onChange={(value) =>
                  onChange((current) => ({ ...current, baseUrl: value }))
                }
              />
              <Field
                label="Username"
                value={form.username}
                placeholder="you@example.com"
                onChange={(value) =>
                  onChange((current) => ({ ...current, username: value }))
                }
              />
            </div>
          </SettingsSection>

          <SettingsSection title="Password">
            {connection.account?.connection.secret.configured && (
              <p className="-mt-1 text-[12px] text-muted-foreground">
                A password is configured. Enter a new one to replace it.
              </p>
            )}

            <Input
              id="account-password"
              type="password"
              className="h-8 rounded-md border-border bg-background text-[13px] shadow-none"
              value={form.password}
              placeholder={
                connection.account?.connection.secret.configured
                  ? '********'
                  : 'Password'
              }
              onChange={(event) =>
                onChange((current) => ({
                  ...current,
                  password: event.target.value,
                }))
              }
            />
          </SettingsSection>
        </>
      )
  }
}

function OAuthConnectionDetails({
  connection,
}: {
  connection: ManagedOAuthConnectionModel
}) {
  const { account } = connection
  return (
    <SettingsSection title="Connection">
      <div className="grid gap-3 sm:grid-cols-2">
        <ReadOnlyDetail
          label="Provider"
          value={providerLabel(account.connection.provider)}
        />
        <ReadOnlyDetail
          label="Authentication"
          value={authLabel(account.connection.auth)}
        />
        <ReadOnlyDetail label="Username" value={account.connection.username} />
        <ReadOnlyDetail label="Driver" value={driverLabel(account.driver)} />
        {account.connection.imap && (
          <ReadOnlyDetail
            label="IMAP"
            value={`${account.connection.imap.host}:${account.connection.imap.port}`}
          />
        )}
        {account.connection.smtp && (
          <ReadOnlyDetail
            label="SMTP"
            value={`${account.connection.smtp.host}:${account.connection.smtp.port}`}
          />
        )}
      </div>
      <p className="text-[12px] leading-5 text-muted-foreground">
        Connection settings and credentials are managed by the provider OAuth
        flow.
      </p>
    </SettingsSection>
  )
}

function ReadOnlyDetail({
  label,
  value,
}: {
  label: string
  value?: string | null
}) {
  return (
    <div className="grid min-h-12 gap-1 rounded-md border border-border-soft bg-bg-elev/45 px-3 py-2">
      <span className="text-[11px] font-medium uppercase tracking-[0.08em] text-muted-foreground">
        {label}
      </span>
      <span className="truncate text-[13px] text-foreground">
        {value?.trim() || 'Not configured'}
      </span>
    </div>
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
      return 'Generic'
  }
}

function authLabel(auth: ProviderAuthKind): string {
  switch (auth) {
    case 'oauth2':
      return 'OAuth 2.0'
    case 'appPassword':
      return 'App password'
    case 'password':
      return 'Password'
  }
}

function driverLabel(driver: AccountOverview['driver']): string {
  switch (driver) {
    case 'jmap':
      return 'JMAP'
    case 'imapSmtp':
      return 'IMAP/SMTP'
    case 'mock':
      return 'Mock'
  }
}

function AccountAppearanceFields({
  accountId,
  form,
  onChange,
  onSaved,
}: {
  accountId: string | null
  form: AccountFormState
  onChange: React.Dispatch<React.SetStateAction<AccountFormState>>
  onSaved: (account: AccountOverview) => Promise<void>
}) {
  const previewAppearance = useMemo(() => appearanceFromForm(form), [form])
  const appearanceKey = accountAppearanceSignature(previewAppearance)
  const savedAppearanceKeyRef = useRef<string | null>(
    accountId ? appearanceKey : null,
  )
  const saveAppearanceMutation = useMutation({
    mutationFn: () =>
      updateAccount(accountId!, {
        appearance: buildAccountAppearanceInput(form),
      }),
    onSuccess: async (account) => {
      savedAppearanceKeyRef.current = accountAppearanceSignature(
        account.appearance,
      )
      await onSaved(account)
    },
  })
  const { error: saveAppearanceError, mutate: saveAppearance } =
    saveAppearanceMutation

  useEffect(() => {
    if (!accountId || appearanceKey === savedAppearanceKeyRef.current) {
      return
    }

    const timeout = window.setTimeout(() => {
      saveAppearance()
    }, 350)
    return () => window.clearTimeout(timeout)
  }, [accountId, appearanceKey, saveAppearance])

  return (
    <div className="grid gap-4 sm:grid-cols-[auto_1fr]">
      <AccountMark
        appearance={previewAppearance}
        className="size-12 rounded-md text-[15px]"
      />

      <div className="min-w-0 space-y-3">
        <div className="grid gap-3 sm:grid-cols-[96px_1fr]">
          <Field
            label="Letter"
            value={form.appearanceInitials}
            onChange={(value) =>
              onChange((current) => ({
                ...current,
                appearanceInitials: value.toUpperCase().slice(0, 1),
              }))
            }
          />
          <label className="grid gap-1.5 text-[13px]">
            <span className="flex items-center justify-between text-[12px] font-medium text-muted-foreground">
              <span>Color</span>
              <span className="font-mono">{form.appearanceColorHue}°</span>
            </span>
            <input
              type="range"
              min={0}
              max={360}
              step={1}
              value={form.appearanceColorHue}
              onChange={(event) =>
                onChange((current) => ({
                  ...current,
                  appearanceColorHue: Number(event.target.value),
                }))
              }
              className="ph-hue-range h-4 w-full cursor-pointer appearance-none rounded-full border border-border-soft bg-transparent accent-primary"
              style={{ background: accountHueGradient }}
            />
          </label>
        </div>
        {saveAppearanceError && (
          <FeedbackBanner tone="error">
            {saveAppearanceError.message}
          </FeedbackBanner>
        )}
      </div>
    </div>
  )
}

function AccountActions({
  account,
  onCommand,
  isCommandPending,
}: {
  account: AccountOverview
  onCommand: (
    action: 'enable' | 'disable' | 'delete' | 'sync',
    account: AccountOverview,
  ) => void
  isCommandPending: boolean
}) {
  return (
    <div className="flex flex-wrap items-center gap-1">
      <Button
        size="sm"
        variant="ghost"
        type="button"
        onClick={() => onCommand('sync', account)}
        disabled={isCommandPending}
      >
        Sync
      </Button>
      <Button
        size="sm"
        variant="ghost"
        type="button"
        onClick={() =>
          onCommand(account.enabled ? 'disable' : 'enable', account)
        }
        disabled={isCommandPending}
      >
        {account.enabled ? 'Disable' : 'Enable'}
      </Button>
    </div>
  )
}

function DangerSection({
  account,
  onCommand,
  isCommandPending,
}: {
  account: AccountOverview
  onCommand: (
    action: 'enable' | 'disable' | 'delete' | 'sync',
    account: AccountOverview,
  ) => void
  isCommandPending: boolean
}) {
  return (
    <SettingsSection title="Danger" tone="danger" className="pt-16">
      <p className="mb-3 text-[12px] text-muted-foreground">
        Remove this account and its synced local data.
      </p>
      <AlertDialog>
        <AlertDialogTrigger asChild>
          <Button
            size="sm"
            variant="destructive"
            type="button"
            disabled={isCommandPending}
          >
            Delete
          </Button>
        </AlertDialogTrigger>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete account?</AlertDialogTitle>
            <AlertDialogDescription>
              This will permanently remove &ldquo;{account.name}&rdquo; and all
              synced data. This cannot be undone.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
              onClick={() => onCommand('delete', account)}
            >
              Delete account
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </SettingsSection>
  )
}
