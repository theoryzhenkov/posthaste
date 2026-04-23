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
import { SectionHeader, StatusDot } from "./shared";
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
    <div className="grid h-full min-h-0 gap-5 px-5 py-5 lg:grid-cols-[18rem_minmax(0,1fr)] lg:px-6 lg:py-6">
      <aside className="flex min-h-0 flex-col overflow-hidden rounded-lg border border-border/80 bg-background/78 shadow-[var(--shadow-pane)]">
        <header className="border-b border-border/80 px-4 py-4">
          <SectionHeader
            eyebrow="Accounts"
            title="Connected sources"
            description="Manage JMAP endpoints, credentials, and sync status."
            actions={
              <Button
                size="sm"
                variant="outline"
                type="button"
                onClick={onCreateAccount}
                className="h-8"
                aria-label="New account"
              >
                <Plus size={13} strokeWidth={2} />
                New
              </Button>
            }
          />
        </header>
        <div className="ph-scroll min-h-0 flex-1 overflow-y-auto p-2">
          {accounts.length === 0 && selectedAccountId !== "new" && (
            <p className="px-4 py-8 text-center text-xs leading-5 text-muted-foreground">
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

      <section className="ph-scroll min-h-0 overflow-y-auto rounded-lg border border-border/80 bg-background/78 shadow-[var(--shadow-pane)]">
        <div className="px-5 py-5 sm:px-6 sm:py-6">
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
      </section>
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
        "flex w-full items-center gap-3 rounded-lg border px-3 py-3 text-left transition-all",
        isActive
          ? "border-primary/25 bg-accent/80 text-accent-foreground shadow-[var(--shadow-pane)]"
          : "border-transparent hover:border-border/80 hover:bg-panel-muted/70",
      )}
    >
      {leading}
      <span className="min-w-0 flex-1">
        <span className="flex items-center gap-1.5">
          <span className="truncate text-sm font-medium">{label}</span>
          {isDefault && (
            <span
              className="shrink-0 rounded-sm bg-background/80 px-1.5 py-0.5 text-[9px] font-mono uppercase tracking-[0.18em] text-muted-foreground"
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
    <div className="flex h-full min-h-[260px] flex-col items-center justify-center rounded-lg border border-dashed border-border/80 bg-panel-muted/40 px-6 text-center">
      <UserPlus size={36} strokeWidth={1.5} className="text-muted-foreground/40" />
      <div className="mt-4">
        <p className="text-sm font-medium">No accounts yet</p>
        <p className="mt-1 text-sm text-muted-foreground">
          Add one to start syncing your mail.
        </p>
      </div>
      <Button size="sm" variant="outline" type="button" onClick={onCreateAccount} className="mt-4">
        <Plus size={13} strokeWidth={2} />
        New account
      </Button>
    </div>
  );
}
