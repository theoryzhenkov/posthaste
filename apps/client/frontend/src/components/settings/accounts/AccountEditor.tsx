/**
 * Account create/edit form with save, verify, and secret management.
 *
 * Saving is a small command sequence: create/update the identity, then (for
 * manual accounts) patch the transport endpoints, then — only when a new
 * password was typed — write the secret through its dedicated command. The
 * backend mints ids, so a create resolves the new account by diffing the
 * accounts answer around the command.
 */
import { useMutation } from '@tanstack/react-query'
import { useMemo, useState } from 'react'

import { classifyAccountSetupError } from '../../../data/models/accountHealth'
import { fetchQuery, useCommands, useMailClient } from '@/data'
import type {
  AccountRow,
  AccountSettingsResult,
  AccountsResult,
  VerifyAccountResult,
} from '@/gen'
import {
  formFieldSetter,
  hasUnsavedAccountChanges,
  imapDefaultsForEmail,
  setupPrimaryEmail,
} from '../forms'
import { AccountMark } from '../../ui/display/AccountMark'
import { Button } from '../../ui/form/button'
import {
  AccountActions,
  type AccountActionTarget,
  type AccountCommandAction,
} from './editor/AccountActions'
import { AccountHealthNotice } from './AccountHealthNotice'
import { AccountAppearanceFields } from './editor/AccountAppearanceFields'
import { AccountHeaderMeta } from './editor/AccountHeaderMeta'
import { ConnectionEditor } from './editor/ConnectionEditor'
import { DangerSection } from './editor/DangerSection'
import {
  buildAccountEditorModel,
  type ExistingAccountEditorModel,
} from './editor/accountEditorModel'
import {
  buildAccountAppearanceInput,
  buildCreateAccountIntent,
  buildIdentityPatch,
  buildSecretChange,
  buildTransportIntent,
  emptyAccountForm,
  formFromAccount,
  shouldWriteTransport,
} from '../forms'
import { SyncProgressMeter } from './list/SyncProgressMeter'
import { FeedbackBanner, Field } from '../panel/shared'
import { SettingsFooter, SettingsPageHeader, SettingsSection } from '../panel/shared'
import type { AccountFormState, EditorTarget } from '../panel/types'

export function AccountEditor({
  editorTarget,
  editingAccount,
  accountRow,
  onSaved,
  onVerified,
  onCommand,
  isCommandPending,
  commandError,
}: {
  editorTarget: EditorTarget
  editingAccount: AccountSettingsResult | null
  /** Live health row for the account being edited (null while loading/new). */
  accountRow: AccountRow | null
  onSaved: (accountId: string) => Promise<void>
  onVerified: () => Promise<void>
  onCommand: (action: AccountCommandAction, account: AccountActionTarget) => void
  isCommandPending: boolean
  commandError: string | null
}) {
  const client = useMailClient()
  const commands = useCommands()
  const editorModel = useMemo(
    () => buildAccountEditorModel(editorTarget, editingAccount),
    [editorTarget, editingAccount],
  )
  const [form, setForm] = useState(() =>
    editingAccount ? formFromAccount(editingAccount) : emptyAccountForm(),
  )
  const appPasswordHint =
    form.driver === 'imapSmtp'
      ? (imapDefaultsForEmail(setupPrimaryEmail(form))?.appPasswordHint ?? null)
      : null
  const [errorMessage, setErrorMessage] = useState<string | null>(null)
  const [verification, setVerification] = useState<VerifyAccountResult | null>(
    null,
  )
  // The saved baseline the form diffs against: untouched fields patch as
  // `keep`, and the Apply gate stays closed until something drifts from it.
  const [savedForm, setSavedForm] = useState(form)

  const saveMutation = useMutation({
    mutationFn: async (currentForm: AccountFormState): Promise<string> => {
      if (editorModel.kind === 'new') {
        const before = new Set(
          (await fetchQuery<AccountsResult>(client, { accounts: {} })).rows.map(
            (row) => row.id,
          ),
        )
        await commands.run({
          createAccount: buildCreateAccountIntent(currentForm),
        })
        const after = await fetchQuery<AccountsResult>(client, {
          accounts: {},
        })
        const created = after.rows.find((row) => !before.has(row.id))
        if (!created) {
          throw new Error(
            'The account was created but its id could not be resolved — reopen it from the accounts list.',
          )
        }
        await commands.run({
          updateAccountTransport: buildTransportIntent(
            currentForm,
            savedForm,
            created.id,
          ),
        })
        const secret = buildSecretChange(currentForm)
        if (secret.kind !== 'keep') {
          await commands.run({
            setAccountSecret: { accountId: created.id, change: secret },
          })
        }
        return created.id
      }

      const accountId = editorModel.account.id
      await commands.run({
        updateAccount: buildIdentityPatch(currentForm, savedForm, accountId),
      })
      if (shouldWriteTransport(editorModel)) {
        await commands.run({
          updateAccountTransport: buildTransportIntent(
            currentForm,
            savedForm,
            accountId,
          ),
        })
        const secret = buildSecretChange(currentForm)
        if (secret.kind !== 'keep') {
          await commands.run({
            setAccountSecret: { accountId, change: secret },
          })
        }
      }
      return accountId
    },
    onSuccess: async (accountId, currentForm) => {
      setErrorMessage(null)
      setVerification(null)
      // The refreshed answer arrives through invalidation; locally only the
      // password field resets (it is write-only) and the saved baseline
      // catches up to what was submitted.
      const nextSaved = { ...currentForm, password: '' }
      setForm(nextSaved)
      setSavedForm(nextSaved)
      await onSaved(accountId)
    },
    onError: (error: Error) => {
      setErrorMessage(classifyAccountSetupError(error, appPasswordHint).message)
    },
  })

  const verifyMutation = useMutation({
    mutationFn: (accountId: string) =>
      fetchQuery<VerifyAccountResult>(client, {
        verifyAccount: { accountId },
      }),
    onSuccess: async (result) => {
      setVerification(result)
      setErrorMessage(null)
      await onVerified()
    },
    onError: (error: Error) => {
      setVerification(null)
      setErrorMessage(classifyAccountSetupError(error, appPasswordHint).message)
    },
  })

  const existingModel: ExistingAccountEditorModel | null =
    editorModel.kind === 'new' ? null : editorModel
  const existingAccount = existingModel?.account ?? null
  const formAppearance = buildAccountAppearanceInput(form)
  const unsavedChanges = hasUnsavedAccountChanges(
    form,
    savedForm,
    editorModel.connection,
  )

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
              <AccountHeaderMeta model={existingModel} row={accountRow} />
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

      {accountRow && (
        <div className="-mt-4 mb-4">
          <SyncProgressMeter account={accountRow} />
        </div>
      )}

      {accountRow && (
        <div className="mt-4">
          <AccountHealthNotice
            account={accountRow}
            onAction={(row) => onCommand('sync', row)}
            isActionPending={isCommandPending}
          />
        </div>
      )}

      <IdentitySection form={form} onChange={setForm} />

      <SettingsSection title="Appearance">
        <AccountAppearanceFields
          accountId={existingAccount?.id ?? null}
          form={form}
          onChange={setForm}
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
            disabled={saveMutation.isPending || !unsavedChanges}
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
                unsavedChanges
              }
              className="rounded-md border-border bg-background"
            >
              Verify connection
            </Button>
          )}
          <span className="text-[12px] text-muted-foreground">
            {unsavedChanges ? 'Unsaved changes' : 'Saved'}
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

function IdentitySection({
  form,
  onChange,
}: {
  form: AccountFormState
  onChange: React.Dispatch<React.SetStateAction<AccountFormState>>
}) {
  const setField = formFieldSetter(onChange)
  return (
    <SettingsSection title="Identity">
      <div className="grid gap-3 sm:grid-cols-2">
        <Field
          label="Account name"
          value={form.name}
          onChange={setField('name')}
        />
        <Field
          label="Full name"
          value={form.fullName}
          placeholder="Ada Lovelace"
          onChange={setField('fullName')}
        />
      </div>

      <label className="grid gap-1.5 text-[13px]">
        <span className="text-[12px] font-medium text-muted-foreground">
          Signature
        </span>
        <textarea
          className="min-h-[72px] w-full resize-y rounded-md border border-border bg-background px-2.5 py-2 text-[13px] shadow-none outline-none placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50"
          value={form.signature}
          placeholder="Optional signature appended to composed messages."
          onChange={(event) => setField('signature')(event.target.value)}
        />
      </label>

      <label className="grid gap-1.5 text-[13px]">
        <span className="text-[12px] font-medium text-muted-foreground">
          Email addresses
        </span>
        <textarea
          className="min-h-[72px] w-full resize-y rounded-md border border-border bg-background px-2.5 py-2 text-[13px] shadow-none outline-none placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50"
          value={form.emailPatternsText}
          placeholder={'you@example.com\n*@example.com'}
          onChange={(event) => setField('emailPatternsText')(event.target.value)}
        />
      </label>
    </SettingsSection>
  )
}
