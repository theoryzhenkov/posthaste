/**
 * Account create/edit form with save, verify, and secret management.
 *
 * @spec docs/L1-api#account-crud-lifecycle
 * @spec docs/L1-api#secret-management
 */
import { useMutation } from '@tanstack/react-query'
import { useState } from 'react'
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
import { Badge } from '../ui/badge'
import { Button } from '../ui/button'
import { Input } from '../ui/input'
import { createAccount, updateAccount, verifyAccount } from '../../api/client'
import type { AccountOverview, VerificationResponse } from '../../api/types'
import { formatRelativeTime } from '../../utils/relativeTime'
import {
  buildCreateAccountPayload,
  buildUpdateAccountPayload,
  EMPTY_FORM,
  formFromAccount,
} from './helpers'
import {
  FeedbackBanner,
  Field,
  SectionCard,
  SectionHeader,
  StatusDot,
} from './shared'
import type { EditorTarget } from './types'

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
  const [form, setForm] = useState(() =>
    editingAccount ? formFromAccount(editingAccount) : EMPTY_FORM,
  )
  const [feedback, setFeedback] = useState<string | null>(null)
  const [errorMessage, setErrorMessage] = useState<string | null>(null)
  const [verification, setVerification] = useState<VerificationResponse | null>(
    null,
  )

  const saveMutation = useMutation({
    mutationFn: (currentForm: typeof form) =>
      editorTarget === 'new'
        ? createAccount(buildCreateAccountPayload(currentForm))
        : updateAccount(editorTarget, buildUpdateAccountPayload(currentForm)),
    onSuccess: async (account) => {
      setFeedback(`Saved ${account.name}.`)
      setErrorMessage(null)
      setVerification(null)
      await onSaved(account)
    },
    onError: (error: Error) => {
      setFeedback(null)
      setErrorMessage(error.message)
    },
  })

  const verifyMutation = useMutation({
    mutationFn: (accountId: string) => verifyAccount(accountId),
    onSuccess: async (result) => {
      setVerification(result)
      setFeedback(
        result.identityEmail
          ? `Verified ${result.identityEmail}.`
          : 'Account verified.',
      )
      setErrorMessage(null)
      await onVerified()
    },
    onError: (error: Error) => {
      setVerification(null)
      setFeedback(null)
      setErrorMessage(error.message)
    },
  })

  const isEditing = editorTarget !== 'new' && editingAccount !== null

  return (
    <div>
      <SectionCard>
        <SectionHeader
          eyebrow="Account editor"
          title={
            editorTarget === 'new'
              ? 'New account'
              : (editingAccount?.name ?? 'Account')
          }
          description={
            editorTarget === 'new'
              ? 'Configure transport details, then save and verify the connection.'
              : 'Update credentials, review sync status, or run account-level actions.'
          }
          actions={
            isEditing && editingAccount ? (
              <AccountActions
                account={editingAccount}
                onCommand={onCommand}
                onVerify={() => verifyMutation.mutate(editingAccount.id)}
                isVerifying={verifyMutation.isPending}
                isCommandPending={isCommandPending}
              />
            ) : null
          }
        />

        {isEditing && editingAccount && (
          <AccountStatusStrip account={editingAccount} />
        )}
      </SectionCard>

      <SectionCard>
        <SectionHeader eyebrow="Identity" title="Mailbox source" />

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
      </SectionCard>

      <SectionCard>
        <SectionHeader eyebrow="Connection" title="Server details" />

        <div className="grid gap-3 sm:grid-cols-2">
          <Field
            label="Base URL"
            value={form.baseUrl}
            placeholder="https://mail.example.com/jmap"
            onChange={(value) =>
              setForm((current) => ({ ...current, baseUrl: value }))
            }
          />
          <Field
            label="Username"
            value={form.username}
            placeholder="you@example.com"
            onChange={(value) =>
              setForm((current) => ({ ...current, username: value }))
            }
          />
        </div>
      </SectionCard>

      <SectionCard>
        <SectionHeader
          eyebrow="Credentials"
          title="Password"
          actions={
            editingAccount?.transport.secret.configured ? (
              <Badge
                variant="outline"
                className="h-6 border-emerald-500/30 bg-emerald-500/10 font-mono text-[10px] uppercase tracking-[0.18em] text-emerald-700"
              >
                configured
              </Badge>
            ) : null
          }
        />

        <Input
          id="account-password"
          type="password"
          className="h-8 rounded-md border-border bg-background text-[13px] shadow-none"
          value={form.password}
          placeholder={
            editingAccount?.transport.secret.configured
              ? '********'
              : 'Password'
          }
          onChange={(event) =>
            setForm((current) => ({
              ...current,
              password: event.target.value,
            }))
          }
        />
      </SectionCard>

      <SectionCard>
        <SectionHeader eyebrow="Changes" title="Apply updates" />

        {feedback && <FeedbackBanner tone="success">{feedback}</FeedbackBanner>}
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

        <div className="flex flex-wrap gap-1.5">
          <Button
            type="button"
            onClick={() => saveMutation.mutate(form)}
            disabled={saveMutation.isPending}
            className="bg-brand-coral text-white hover:bg-brand-coral/90"
          >
            Apply
          </Button>
        </div>
      </SectionCard>
    </div>
  )
}

function AccountActions({
  account,
  onCommand,
  onVerify,
  isVerifying,
  isCommandPending,
}: {
  account: AccountOverview
  onCommand: (
    action: 'enable' | 'disable' | 'delete' | 'sync',
    account: AccountOverview,
  ) => void
  onVerify: () => void
  isVerifying: boolean
  isCommandPending: boolean
}) {
  return (
    <div className="flex flex-wrap items-center gap-2">
      <Button
        size="sm"
        variant="outline"
        type="button"
        onClick={onVerify}
        disabled={isVerifying}
      >
        Verify
      </Button>
      <Button
        size="sm"
        variant="outline"
        type="button"
        onClick={() => onCommand('sync', account)}
        disabled={isCommandPending}
      >
        Sync
      </Button>
      <Button
        size="sm"
        variant="outline"
        type="button"
        onClick={() =>
          onCommand(account.enabled ? 'disable' : 'enable', account)
        }
        disabled={isCommandPending}
      >
        {account.enabled ? 'Disable' : 'Enable'}
      </Button>
      <AlertDialog>
        <AlertDialogTrigger asChild>
          <Button size="sm" variant="destructive" type="button">
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
    </div>
  )
}

function AccountStatusStrip({ account }: { account: AccountOverview }) {
  return (
    <>
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1.5 rounded-md border border-border-soft bg-bg-elev px-3 py-2 text-[12px] text-muted-foreground">
        <span className="flex items-center gap-1.5">
          <StatusDot status={account.status} />
          <span className="font-mono uppercase tracking-wider">
            {account.status}
          </span>
        </span>
        <span>
          Last sync:{' '}
          {account.lastSyncAt
            ? formatRelativeTime(account.lastSyncAt)
            : 'never'}
        </span>
        <span>Real-time: {account.push}</span>
      </div>
      {account.lastSyncError && (
        <p className="mt-2 rounded-md border border-destructive/20 bg-destructive/5 px-3 py-2 text-[12px] text-destructive">
          {account.lastSyncError}
        </p>
      )}
    </>
  )
}
