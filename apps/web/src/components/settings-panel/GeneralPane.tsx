/**
 * General preferences: default account selector and at-a-glance overview.
 *
 * @spec docs/L1-api#account-crud-lifecycle
 */
import type { AccountOverview } from '../../api/types'
import { isTauriRuntime } from '../../desktop'
import {
  setDeveloperToolsEnabled,
  useDeveloperToolsEnabled,
} from '../../developerTools'
import { cn } from '../../lib/utils'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '../ui/select'
import { SettingsPage, SettingsPageHeader, SettingsSection } from './shared'
import { OutboxSection } from './OutboxSection'
import { TroubleshootingSection } from './TroubleshootingSection'
import { UpdatesSection } from './UpdatesSection'

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
  const developerToolsEnabled = useDeveloperToolsEnabled()

  return (
    <SettingsPage>
      <SettingsPageHeader
        title="General"
        description="Choose the default account Posthaste should use when no source is selected."
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

      <OutboxSection />

      {isTauriRuntime() && <UpdatesSection />}

      {isTauriRuntime() && <TroubleshootingSection />}

      {isTauriRuntime() && (
        <SettingsSection title="Developer">
          <div className="grid gap-3 sm:grid-cols-[1fr_280px] sm:items-center">
            <div className="min-w-0">
              <p className="text-[13px] font-medium text-foreground">
                Developer tools
              </p>
              <p className="mt-1 text-[12px] leading-5 text-muted-foreground">
                Enable the in-app web inspector, toggled with ⌘⌥I. Off by
                default.
              </p>
            </div>
            <button
              type="button"
              role="switch"
              aria-checked={developerToolsEnabled}
              aria-label="Developer tools"
              onClick={() => setDeveloperToolsEnabled(!developerToolsEnabled)}
              className={cn(
                'ph-focus-ring relative inline-flex h-5 w-9 shrink-0 items-center rounded-full transition-colors sm:justify-self-end',
                developerToolsEnabled
                  ? 'bg-[var(--brand-coral)]'
                  : 'bg-[color-mix(in_oklab,var(--foreground)_22%,transparent)]',
              )}
            >
              <span
                className={cn(
                  'inline-block size-4 rounded-full bg-white shadow-sm transition-transform',
                  developerToolsEnabled ? 'translate-x-4' : 'translate-x-0.5',
                )}
              />
            </button>
          </div>
        </SettingsSection>
      )}
    </SettingsPage>
  )
}
