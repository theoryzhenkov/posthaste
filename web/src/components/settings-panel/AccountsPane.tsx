/**
 * Accounts view: slim account list paired with the account editor.
 *
 * @spec docs/L1-api#account-crud-lifecycle
 */
import type { UseMutationResult } from "@tanstack/react-query";
import { Plus, UserPlus } from "lucide-react";
import type { AccountOverview } from "../../api/types";
import { cn } from "../../lib/utils";
import { AccountEditor } from "./AccountEditor";
import { Button } from "../ui/button";
import { StatusDot } from "./shared";
import type { EditorTarget } from "./types";

export function AccountsPane({
  accounts,
  selectedAccountId,
  editingAccount,
  editorKey,
  onSelectAccount,
  onCreateAccount,
  onCommand,
  onSaved,
  onVerified,
  commandMutation,
}: {
  accounts: AccountOverview[];
  selectedAccountId: EditorTarget;
  editingAccount: AccountOverview | null;
  editorKey: string;
  onSelectAccount: (accountId: string) => void;
  onCreateAccount: () => void;
  onCommand: (
    action: "enable" | "disable" | "delete" | "sync",
    account: AccountOverview,
  ) => void;
  onSaved: (account: AccountOverview) => Promise<void>;
  onVerified: () => Promise<void>;
  commandMutation: UseMutationResult<
    unknown,
    Error,
    { action: "enable" | "disable" | "delete" | "sync"; account: AccountOverview },
    unknown
  >;
}) {
  return (
    <div className="grid h-full min-h-0 grid-cols-[220px_minmax(0,1fr)]">
      <aside className="flex min-h-0 flex-col border-r border-border bg-background/30">
        <header className="flex items-center justify-between gap-2 border-b border-border px-3 py-2">
          <span className="text-[10px] font-mono uppercase tracking-[0.2em] text-muted-foreground">
            accounts
          </span>
          <Button
            size="xs"
            variant="ghost"
            type="button"
            onClick={onCreateAccount}
            className="-mr-1 h-6 gap-1 px-1.5 text-xs"
            aria-label="New account"
          >
            <Plus size={13} strokeWidth={2} />
            New
          </Button>
        </header>
        <div className="min-h-0 flex-1 overflow-y-auto py-1">
          {accounts.length === 0 && selectedAccountId !== "new" && (
            <p className="px-3 py-6 text-center text-xs text-muted-foreground">
              No accounts yet.
            </p>
          )}
          {selectedAccountId === "new" && (
            <AccountListRow
              isActive
              label="New account"
              sublabel="Unsaved"
              leading={
                <span className="flex h-2 w-2 items-center justify-center">
                  <span className="h-1.5 w-1.5 rounded-full border border-dashed border-muted-foreground" />
                </span>
              }
              onClick={() => undefined}
            />
          )}
          {accounts.map((account) => (
            <AccountListRow
              key={account.id}
              isActive={selectedAccountId === account.id}
              label={account.name}
              sublabel={
                account.transport.username ?? account.driver.toUpperCase()
              }
              isDefault={account.isDefault}
              leading={<StatusDot status={account.status} />}
              onClick={() => onSelectAccount(account.id)}
            />
          ))}
        </div>
      </aside>

      <div className="min-h-0 overflow-y-auto px-6 py-6">
        {selectedAccountId === "new" || editingAccount ? (
          <AccountEditor
            key={editorKey}
            editorTarget={selectedAccountId}
            editingAccount={editingAccount}
            onSaved={onSaved}
            onVerified={onVerified}
            onCommand={onCommand}
            isCommandPending={commandMutation.isPending}
          />
        ) : (
          <AccountsEmptyState onCreateAccount={onCreateAccount} />
        )}
      </div>
    </div>
  );
}

function AccountListRow({
  isActive,
  label,
  sublabel,
  isDefault,
  leading,
  onClick,
}: {
  isActive: boolean;
  label: string;
  sublabel?: string;
  isDefault?: boolean;
  leading: React.ReactNode;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "flex w-full items-center gap-2.5 px-3 py-2 text-left transition-colors",
        isActive
          ? "bg-accent text-accent-foreground"
          : "hover:bg-accent/50",
      )}
    >
      {leading}
      <span className="min-w-0 flex-1">
        <span className="flex items-center gap-1.5">
          <span className="truncate text-sm font-medium">{label}</span>
          {isDefault && (
            <span
              className="shrink-0 text-[9px] font-mono uppercase tracking-wider text-muted-foreground"
              title="Default account"
            >
              default
            </span>
          )}
        </span>
        {sublabel && (
          <span className="block truncate text-xs text-muted-foreground">
            {sublabel}
          </span>
        )}
      </span>
    </button>
  );
}

function AccountsEmptyState({
  onCreateAccount,
}: {
  onCreateAccount: () => void;
}) {
  return (
    <div className="flex h-full min-h-[200px] flex-col items-center justify-center gap-4 text-center">
      <UserPlus size={36} strokeWidth={1.5} className="text-muted-foreground/40" />
      <div>
        <p className="text-sm font-medium">No accounts yet</p>
        <p className="mt-1 text-xs text-muted-foreground">
          Add one to start syncing your mail.
        </p>
      </div>
      <Button size="sm" variant="outline" type="button" onClick={onCreateAccount}>
        <Plus size={13} strokeWidth={2} />
        New account
      </Button>
    </div>
  );
}
