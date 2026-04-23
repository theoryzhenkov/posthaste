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
import { SectionCard, SectionHeader, SummaryCard } from "./shared";

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
    <div className="mx-auto flex max-w-[56rem] flex-col gap-5">
      <SectionHeader
        eyebrow="Workspace defaults"
        title="General settings"
        description="Choose the account PostHaste should fall back to and keep a quick read on account and mailbox coverage."
      />

      <div className="grid gap-5 xl:grid-cols-[minmax(0,24rem)_minmax(0,1fr)]">
        <SectionCard className="space-y-4">
          <SectionHeader
            eyebrow="Sending"
            title="Default account"
            description="Used when a compose flow does not already have account context."
          />

          <label className="grid gap-2 text-sm">
            <span className="text-[11px] font-medium text-muted-foreground">
              Account
            </span>
            <Select
              value={defaultAccountId ?? "__none__"}
              onValueChange={(value) =>
                onDefaultAccountChange(value === "__none__" ? null : value)
              }
              disabled={isPending}
            >
              <SelectTrigger className="h-9 w-full border-border/80 bg-panel shadow-none">
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
            <span className="text-xs leading-5 text-muted-foreground">
              New messages use this account unless the current mailbox or thread
              already implies another source.
            </span>
          </label>
        </SectionCard>

        <SectionCard className="space-y-4">
          <SectionHeader
            eyebrow="Overview"
            title="At a glance"
            description="Current connected-source and smart-mailbox coverage."
          />
          <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
            <SummaryCard label="Accounts" value={String(accounts.length)} />
            <SummaryCard label="Ready" value={String(readyCount)} />
            <SummaryCard label="Enabled" value={String(enabledCount)} />
            <SummaryCard label="Mailboxes" value={String(smartMailboxes.length)} />
          </div>
        </SectionCard>
      </div>
    </div>
  );
}
