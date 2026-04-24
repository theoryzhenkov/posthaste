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
    <div className="mx-auto flex max-w-[760px] flex-col">
      <header>
        <h1 className="text-[24px] font-semibold leading-tight text-foreground">
          General
        </h1>
        <p className="mt-2 max-w-[620px] text-[13px] leading-6 text-muted-foreground">
          Choose the default account PostHaste should use when no source is
          selected.
        </p>
      </header>

      <div className="mt-7">
        <section>
          <h2 className="mb-2 text-[13px] font-semibold text-foreground">
            Workspace defaults
          </h2>
          <div className="overflow-hidden rounded-lg border border-border-soft bg-bg-elev/45">
            <div className="flex min-h-[60px] flex-col gap-3 px-4 py-3 sm:flex-row sm:items-center sm:justify-between">
              <div className="min-w-0">
                <p className="text-[13px] font-medium text-foreground">
                  Default account
                </p>
                <p className="mt-1 text-[12px] leading-5 text-muted-foreground">
                  Used when a compose flow does not already have account
                  context.
                </p>
              </div>
              <div className="w-full shrink-0 sm:w-[280px]">
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
            </div>
          </div>
        </section>
      </div>
    </div>
  )
}
