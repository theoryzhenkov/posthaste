/**
 * General preferences: default account selector and at-a-glance overview.
 *
 * @spec docs/L1-api#account-crud-lifecycle
 */
import type { AccountOverview, SmartMailboxSummary } from "../../api/types";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "../ui/select";
import { SectionHeader, SummaryCard } from "./shared";

export function GeneralPane({
  accounts,
  smartMailboxes,
  defaultAccountId,
  onDefaultAccountChange,
  isPending,
}: {
  accounts: AccountOverview[];
  smartMailboxes: SmartMailboxSummary[];
  defaultAccountId: string | null | undefined;
  onDefaultAccountChange: (accountId: string | null) => void;
  isPending: boolean;
}) {
  const readyCount = accounts.filter((account) => account.status === "ready").length;
  const enabledCount = accounts.filter((account) => account.enabled).length;

  return (
    <div className="space-y-8">
      <section className="space-y-4">
        <SectionHeader
          title="General"
          description="Defaults that apply across all accounts."
        />

        <label className="grid max-w-md gap-1.5 text-sm">
          <span className="text-muted-foreground">Default account</span>
          <Select
            value={defaultAccountId ?? "__none__"}
            onValueChange={(value) =>
              onDefaultAccountChange(value === "__none__" ? null : value)
            }
            disabled={isPending}
          >
            <SelectTrigger className="h-9 w-full">
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
          <span className="text-xs text-muted-foreground">
            Used as the sending account for new messages when no context is set.
          </span>
        </label>
      </section>

      <section className="space-y-4">
        <SectionHeader title="At a glance" />
        <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
          <SummaryCard label="Accounts" value={String(accounts.length)} />
          <SummaryCard label="Ready" value={String(readyCount)} />
          <SummaryCard label="Enabled" value={String(enabledCount)} />
          <SummaryCard label="Mailboxes" value={String(smartMailboxes.length)} />
        </div>
      </section>
    </div>
  );
}
