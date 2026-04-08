/**
 * Left-pane account list with default-account selector, summary cards,
 * and per-account action buttons (edit, sync, enable/disable, delete).
 *
 * @spec docs/L1-api#account-crud-lifecycle
 */
import type { UseMutationResult } from "@tanstack/react-query";
import { UserPlus } from "lucide-react";
import type { AccountOverview } from "../../api/types";
import { cn } from "../../lib/utils";
import { formatRelativeTime } from "../../utils/relativeTime";
import { statusTone } from "./helpers";
import { MetaStat, SummaryCard } from "./shared";
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
} from "../ui/alert-dialog";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "../ui/select";

/**
 * Account list pane: default account selector, summary stats, and account cards.
 *
 * @spec docs/L1-api#account-crud-lifecycle
 */
export function AccountListPane({
  accounts,
  selectedAccountId,
  defaultAccountId,
  accountSummary,
  onDefaultAccountChange,
  onCreateAccount,
  onSelectAccount,
  onCommand,
  defaultMutation,
}: {
  accounts: AccountOverview[];
  selectedAccountId: string | "new";
  defaultAccountId: string | null | undefined;
  accountSummary: {
    total: number;
    readyCount: number;
    degradedCount: number;
    enabledCount: number;
  };
  onDefaultAccountChange: (accountId: string | null) => void;
  onCreateAccount: () => void;
  onSelectAccount: (accountId: string) => void;
  onCommand: (
    action: "enable" | "disable" | "delete" | "sync",
    account: AccountOverview,
  ) => void;
  defaultMutation: UseMutationResult<unknown, Error, string | null, unknown>;
}) {
  return (
    <>
      <section className="border-b border-border px-4 py-4">
        <p className="text-[10px] font-mono uppercase tracking-[0.24em] text-muted-foreground">
          application
        </p>
        <div className="mt-3 grid gap-3">
          <div className="grid gap-1.5 text-sm">
            <span className="text-muted-foreground">Default account</span>
            <Select
              value={defaultAccountId ?? "__none__"}
              onValueChange={(value) =>
                onDefaultAccountChange(value === "__none__" ? null : value)
              }
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
          </div>

          <div className="grid grid-cols-2 gap-2">
            <SummaryCard label="Accounts" value={String(accountSummary.total)} />
            <SummaryCard label="Ready" value={String(accountSummary.readyCount)} />
            <SummaryCard
              label="Needs attention"
              value={String(accountSummary.degradedCount)}
            />
            <SummaryCard label="Enabled" value={String(accountSummary.enabledCount)} />
          </div>
        </div>
      </section>

      <section className="px-4 py-4">
        <div className="flex items-center justify-between gap-3">
          <p className="text-[10px] font-mono uppercase tracking-[0.24em] text-muted-foreground">
            configured accounts
          </p>
          <Button
            size="sm"
            variant="outline"
            type="button"
            onClick={onCreateAccount}
            disabled={defaultMutation.isPending}
          >
            New account
          </Button>
        </div>
        <div className="mt-3 space-y-3">
          {accounts.length === 0 && (
            <div className="flex flex-col items-center gap-3 rounded-lg border border-dashed border-border px-4 py-8">
              <UserPlus size={32} strokeWidth={1.5} className="text-muted-foreground/40" />
              <div className="text-center">
                <p className="text-sm font-medium text-muted-foreground">No accounts yet</p>
                <p className="mt-1 text-xs text-muted-foreground/60">
                  Create one to start syncing your mail
                </p>
              </div>
            </div>
          )}
          {accounts.map((account) => (
            <article
              key={account.id}
              className={cn(
                "rounded-lg border border-border bg-background/60 px-3 py-3",
                selectedAccountId === account.id &&
                  "border-primary/60 shadow-[0_0_0_1px_rgba(37,99,235,0.25)]",
              )}
            >
              <div className="flex items-start justify-between gap-3">
                <button
                  type="button"
                  className="min-w-0 text-left"
                  onClick={() => onSelectAccount(account.id)}
                >
                  <div className="flex items-center gap-2">
                    <span className="truncate text-sm font-medium">{account.name}</span>
                    {account.isDefault && (
                      <Badge
                        variant="outline"
                        className="border-primary/40 bg-primary/10 font-mono text-[10px] uppercase tracking-wider text-primary"
                      >
                        default
                      </Badge>
                    )}
                  </div>
                  <p className="mt-1 text-xs text-muted-foreground">
                    {account.transport.username ?? "No username"} ·{" "}
                    {account.driver.toUpperCase()}
                  </p>
                </button>

                <Badge
                  variant="outline"
                  className={cn(
                    "font-mono text-[10px] uppercase tracking-wider",
                    statusTone(account.status),
                  )}
                >
                  {account.status}
                </Badge>
              </div>

              <dl className="mt-3 grid grid-cols-2 gap-2 text-xs text-muted-foreground">
                <MetaStat label="Push" value={account.push} />
                <MetaStat
                  label="Password"
                  value={account.transport.secret.configured ? "configured" : "missing"}
                />
                <MetaStat
                  label="Last sync"
                  value={account.lastSyncAt ? formatRelativeTime(account.lastSyncAt) : "never"}
                />
                <MetaStat label="Enabled" value={account.enabled ? "yes" : "no"} />
              </dl>

              {account.lastSyncError && (
                <p className="mt-3 rounded border border-destructive/20 bg-destructive/5 px-2 py-1.5 text-xs text-destructive">
                  {account.lastSyncError}
                </p>
              )}

              <div className="mt-3 flex flex-wrap gap-2">
                <Button
                  size="xs"
                  variant="outline"
                  type="button"
                  onClick={() => onSelectAccount(account.id)}
                >
                  Edit
                </Button>
                <Button
                  size="xs"
                  variant="outline"
                  type="button"
                  onClick={() => onCommand("sync", account)}
                >
                  Sync
                </Button>
                <Button
                  size="xs"
                  variant="outline"
                  type="button"
                  onClick={() =>
                    onCommand(account.enabled ? "disable" : "enable", account)
                  }
                >
                  {account.enabled ? "Disable" : "Enable"}
                </Button>
                <AlertDialog>
                  <AlertDialogTrigger asChild>
                    <Button size="xs" variant="destructive" type="button">
                      Delete
                    </Button>
                  </AlertDialogTrigger>
                  <AlertDialogContent>
                    <AlertDialogHeader>
                      <AlertDialogTitle>Delete account?</AlertDialogTitle>
                      <AlertDialogDescription>
                        This will permanently remove &ldquo;{account.name}&rdquo; and all synced
                        data. This cannot be undone.
                      </AlertDialogDescription>
                    </AlertDialogHeader>
                    <AlertDialogFooter>
                      <AlertDialogCancel>Cancel</AlertDialogCancel>
                      <AlertDialogAction
                        variant="destructive"
                        onClick={() => onCommand("delete", account)}
                      >
                        Delete account
                      </AlertDialogAction>
                    </AlertDialogFooter>
                  </AlertDialogContent>
                </AlertDialog>
              </div>
            </article>
          ))}
        </div>
      </section>
    </>
  );
}
