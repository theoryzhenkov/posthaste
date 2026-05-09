/**
 * General preferences: default account selector and beta telemetry consent.
 *
 * @spec docs/L1-api#account-crud-lifecycle
 * @spec docs/L1-telemetry#consent
 */
import type {
  AccountOverview,
  TelemetryMode,
  TelemetrySettings,
} from '../../api/types'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '../ui/select'
import { SettingsPage, SettingsPageHeader, SettingsSection } from './shared'

export function GeneralPane({
  accounts,
  defaultAccountId,
  telemetry,
  onDefaultAccountChange,
  onTelemetryModeChange,
  isPending,
}: {
  accounts: AccountOverview[]
  defaultAccountId: string | null | undefined
  telemetry: TelemetrySettings | undefined
  onDefaultAccountChange: (accountId: string | null) => void
  onTelemetryModeChange: (mode: TelemetryMode) => void
  isPending: boolean
}) {
  return (
    <SettingsPage>
      <SettingsPageHeader
        title="General"
        description="Choose the default account and beta data-sharing preferences."
      />

      <SettingsSection title="Defaults">
        <div className="grid gap-3 sm:grid-cols-[1fr_280px] sm:items-center">
          <div className="min-w-0">
            <p className="text-[13px] font-medium text-foreground">
              Default account
            </p>
            <p className="mt-1 text-[12px] leading-5 text-muted-foreground">
              Used when a compose flow does not already have account context.
            </p>
          </div>
          <Select
            value={defaultAccountId ?? '__none__'}
            onValueChange={(value) =>
              onDefaultAccountChange(value === '__none__' ? null : value)
            }
            disabled={isPending}
          >
            <SelectTrigger className="h-8 w-full rounded-md border-border bg-background text-[13px] shadow-none">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="__none__">No default</SelectItem>
              {accounts.map((account) => (
                <SelectItem key={account.id} value={account.id}>
                  {account.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </SettingsSection>

      <SettingsSection title="Beta telemetry">
        <div className="grid gap-3 sm:grid-cols-[1fr_280px] sm:items-start">
          <div className="min-w-0">
            <p className="text-[13px] font-medium text-foreground">
              Data sharing
            </p>
            <p className="mt-1 text-[12px] leading-5 text-muted-foreground">
              Optional beta telemetry helps us see startup, sync, send, search,
              cache, and UI health. It never uploads mail content, addresses,
              search text, mailbox names, local logs, console output, or stack
              traces.
            </p>
          </div>
          <Select
            value={telemetry?.mode ?? 'off'}
            onValueChange={(value) =>
              onTelemetryModeChange(value as TelemetryMode)
            }
            disabled={isPending}
          >
            <SelectTrigger className="h-8 w-full rounded-md border-border bg-background text-[13px] shadow-none">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="off">Off</SelectItem>
              <SelectItem value="aggregate">
                Anonymous health telemetry
              </SelectItem>
              <SelectItem value="product">Product analytics</SelectItem>
            </SelectContent>
          </Select>
        </div>
        {telemetry?.mode === 'product' && (
          <p className="mt-3 rounded-md border border-amber-500/25 bg-amber-500/8 px-3 py-2 text-[12px] leading-5 text-muted-foreground">
            Product analytics may use a rotating pseudonymous monthly ID for
            repeated-failure and active-install trends. Choose anonymous health
            telemetry if you do not want that identifier.
          </p>
        )}
      </SettingsSection>
    </SettingsPage>
  )
}
