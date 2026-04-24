/**
 * General preferences: default account selector and at-a-glance overview.
 *
 * @spec docs/L1-api#account-crud-lifecycle
 */
import type { AccountOverview } from '../../api/types'
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
  onDefaultAccountChange,
  isPending,
}: {
  accounts: AccountOverview[]
  defaultAccountId: string | null | undefined
  onDefaultAccountChange: (accountId: string | null) => void
  isPending: boolean
}) {
  return (
    <SettingsPage>
      <SettingsPageHeader
        title="General"
        description="Choose the default account PostHaste should use when no source is selected."
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
    </SettingsPage>
  )
}
