import type { Dispatch, SetStateAction } from 'react'

import { Input } from '../../ui/input'
import type {
  AccountEditorConnectionModel,
  ManagedOAuthConnectionModel,
} from '../accountEditorModel'
import { Field, SettingsSection } from '../shared'
import type { AccountFormState } from '../types'
import { authLabel, driverLabel, providerLabel } from './labels'

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
          value={providerLabel(account.connection.providerKind)}
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
