import type { Dispatch, SetStateAction } from 'react'

import type { TransportSecurity } from '@/gen'
import { Input } from '../../../ui/form/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '../../../ui/form/select'
import type {
  AccountEditorConnectionModel,
  ManagedOAuthConnectionModel,
} from './accountEditorModel'
import { applyImapDefaults, imapDefaultsForEmail } from '../../forms'
import { Field, SettingsSection } from '../../panel/shared'
import type { AccountFormState, ManualAccountDriver } from '../../panel/types'
import { authLabel, driverLabel, providerLabel } from './labels'

const SECURITY_OPTIONS: { value: TransportSecurity; label: string }[] = [
  { value: 'tls', label: 'SSL/TLS' },
  { value: 'startTls', label: 'STARTTLS' },
  { value: 'plain', label: 'None (plain)' },
]

export function ConnectionEditor({
  connection,
  form,
  onChange,
}: {
  connection: AccountEditorConnectionModel
  form: AccountFormState
  onChange: Dispatch<SetStateAction<AccountFormState>>
}) {
  switch (connection.kind) {
    case 'managedOAuth':
      return <OAuthConnectionDetails connection={connection} />
    case 'manualCredentials':
      return (
        <ManualConnectionEditor
          connection={connection}
          form={form}
          onChange={onChange}
        />
      )
  }
}

function ManualConnectionEditor({
  connection,
  form,
  onChange,
}: {
  connection: Extract<
    AccountEditorConnectionModel,
    { kind: 'manualCredentials' }
  >
  form: AccountFormState
  onChange: Dispatch<SetStateAction<AccountFormState>>
}) {
  const isImap = form.driver === 'imapSmtp'
  const appPasswordHint = isImap
    ? (imapDefaultsForEmail(primaryEmail(form))?.appPasswordHint ?? null)
    : null

  return (
    <>
      <SettingsSection title="Connection">
        <DriverPicker
          value={form.driver}
          onChange={(driver) =>
            onChange((current) => {
              const next = { ...current, driver }
              return driver === 'imapSmtp' ? applyImapDefaults(next) : next
            })
          }
        />
      </SettingsSection>

      {isImap ? (
        <ImapServerFields form={form} onChange={onChange} />
      ) : (
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
      )}

      <SettingsSection title="Password">
        {connection.account?.transport.secret.configured && (
          <p className="-mt-1 text-[12px] text-muted-foreground">
            A password is configured. Enter a new one to replace it.
          </p>
        )}
        {appPasswordHint && (
          <p className="-mt-1 text-[12px] text-muted-foreground">
            {appPasswordHint}
          </p>
        )}

        <Input
          id="account-password"
          type="password"
          className="h-8 rounded-md border-border bg-background text-[13px] shadow-none"
          value={form.password}
          placeholder={
            connection.account?.transport.secret.configured
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

function DriverPicker({
  value,
  onChange,
}: {
  value: ManualAccountDriver
  onChange: (driver: ManualAccountDriver) => void
}) {
  const options: { value: ManualAccountDriver; label: string }[] = [
    { value: 'jmap', label: 'JMAP' },
    { value: 'imapSmtp', label: 'IMAP / SMTP' },
  ]
  return (
    <div className="grid gap-1.5 text-[13px]">
      <span className="text-[12px] font-medium text-muted-foreground">
        Protocol
      </span>
      <div
        role="radiogroup"
        aria-label="Account protocol"
        className="inline-flex w-fit rounded-md border border-border bg-background p-0.5"
      >
        {options.map((option) => {
          const active = option.value === value
          return (
            <button
              key={option.value}
              type="button"
              role="radio"
              aria-checked={active}
              onClick={() => onChange(option.value)}
              className={`rounded-[5px] px-3 py-1 text-[12px] font-medium transition-colors ${
                active
                  ? 'bg-brand-coral text-white'
                  : 'text-muted-foreground hover:text-foreground'
              }`}
            >
              {option.label}
            </button>
          )
        })}
      </div>
    </div>
  )
}

function ImapServerFields({
  form,
  onChange,
}: {
  form: AccountFormState
  onChange: Dispatch<SetStateAction<AccountFormState>>
}) {
  return (
    <>
      <SettingsSection title="Incoming (IMAP)">
        <div className="grid gap-3 sm:grid-cols-2">
          <Field
            label="Username"
            value={form.username}
            placeholder="you@example.com"
            onChange={(value) =>
              onChange((current) => ({ ...current, username: value }))
            }
          />
          <div />
          <Field
            label="IMAP host"
            value={form.imapHost}
            placeholder="imap.example.com"
            onChange={(value) =>
              onChange((current) => ({ ...current, imapHost: value }))
            }
          />
          <div className="grid grid-cols-2 gap-3">
            <Field
              label="Port"
              value={form.imapPort}
              placeholder="993"
              onChange={(value) =>
                onChange((current) => ({ ...current, imapPort: value }))
              }
            />
            <SecurityField
              label="Security"
              value={form.imapSecurity}
              onChange={(security) =>
                onChange((current) => ({ ...current, imapSecurity: security }))
              }
            />
          </div>
        </div>
      </SettingsSection>

      <SettingsSection title="Outgoing (SMTP)">
        <div className="grid gap-3 sm:grid-cols-2">
          <Field
            label="SMTP host"
            value={form.smtpHost}
            placeholder="smtp.example.com"
            onChange={(value) =>
              onChange((current) => ({ ...current, smtpHost: value }))
            }
          />
          <div className="grid grid-cols-2 gap-3">
            <Field
              label="Port"
              value={form.smtpPort}
              placeholder="465"
              onChange={(value) =>
                onChange((current) => ({ ...current, smtpPort: value }))
              }
            />
            <SecurityField
              label="Security"
              value={form.smtpSecurity}
              onChange={(security) =>
                onChange((current) => ({ ...current, smtpSecurity: security }))
              }
            />
          </div>
        </div>
      </SettingsSection>
    </>
  )
}

function SecurityField({
  label,
  value,
  onChange,
}: {
  label: string
  value: TransportSecurity
  onChange: (value: TransportSecurity) => void
}) {
  return (
    <label className="grid gap-1.5 text-[13px]">
      <span className="text-[12px] font-medium text-muted-foreground">
        {label}
      </span>
      <Select
        value={value}
        onValueChange={(next) => onChange(next as TransportSecurity)}
      >
        <SelectTrigger
          aria-label={label}
          className="h-8 rounded-md border-border bg-background text-[13px] shadow-none"
        >
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {SECURITY_OPTIONS.map((option) => (
            <SelectItem key={option.value} value={option.value}>
              {option.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </label>
  )
}

function primaryEmail(form: AccountFormState): string {
  const fromPatterns = form.emailPatternsText
    .split(/[\n,]/)
    .map((pattern) => pattern.trim())
    .find((pattern) => !pattern.includes('*') && pattern.includes('@'))
  if (fromPatterns) {
    return fromPatterns
  }
  return form.username.trim()
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
          value={providerLabel(account.transport.provider)}
        />
        <ReadOnlyDetail
          label="Authentication"
          value={authLabel(account.transport.auth)}
        />
        <ReadOnlyDetail label="Username" value={account.transport.username} />
        <ReadOnlyDetail label="Driver" value={driverLabel(account.driver)} />
        {account.transport.imap && (
          <ReadOnlyDetail
            label="IMAP"
            value={`${account.transport.imap.host}:${account.transport.imap.port}`}
          />
        )}
        {account.transport.smtp && (
          <ReadOnlyDetail
            label="SMTP"
            value={`${account.transport.smtp.host}:${account.transport.smtp.port}`}
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
