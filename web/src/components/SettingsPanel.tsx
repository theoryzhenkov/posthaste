/**
 * Full-screen settings panel for account and smart mailbox administration.
 *
 * Two-column layout: left pane lists accounts and smart mailboxes;
 * right pane shows the active editor form.
 *
 * @spec docs/L1-api#account-crud-lifecycle
 * @spec docs/L1-api#smart-mailbox-crud
 */
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import {
  deleteAccount,
  deleteSmartMailbox,
  disableAccount,
  enableAccount,
  fetchAccount,
  fetchSettings,
  fetchSmartMailbox,
  fetchSmartMailboxes,
  patchSettings,
  resetDefaultSmartMailboxes,
  triggerSync,
  updateSmartMailbox,
} from "../api/client";
import type {
  AccountOverview,
  SmartMailboxSummary,
} from "../api/types";
import { AccountEditor } from "./settings-panel/AccountEditor";
import { AccountListPane } from "./settings-panel/AccountListPane";
import { SmartMailboxEditor } from "./settings-panel/SmartMailboxEditor";
import { SmartMailboxListPane } from "./settings-panel/SmartMailboxListPane";
import type {
  EditorTarget,
  SmartMailboxEditorTarget,
} from "./settings-panel/types";

/** @spec docs/L1-api#account-crud-lifecycle */
interface SettingsPanelProps {
  accounts: AccountOverview[];
  activeAccountId: string | null;
  onActiveAccountChange: (accountId: string | null) => void;
}

/**
 * Settings panel with account CRUD, smart mailbox CRUD, and default-account selection.
 *
 * @spec docs/L1-api#account-crud-lifecycle
 * @spec docs/L1-api#smart-mailbox-crud
 */
export function SettingsPanel({
  accounts,
  activeAccountId,
  onActiveAccountChange,
}: SettingsPanelProps) {
  const queryClient = useQueryClient();
  const [editorTarget, setEditorTarget] = useState<EditorTarget>(
    accounts[0]?.id ?? "new",
  );
  const [smartMailboxEditorTarget, setSmartMailboxEditorTarget] =
    useState<SmartMailboxEditorTarget>("new");
  const [smartMailboxActionPendingKey, setSmartMailboxActionPendingKey] =
    useState<string | null>(null);
  const [smartMailboxActionError, setSmartMailboxActionError] = useState<string | null>(null);

  const settingsQuery = useQuery({
    queryKey: ["settings"],
    queryFn: fetchSettings,
  });
  const smartMailboxListQuery = useQuery({
    queryKey: ["smart-mailboxes"],
    queryFn: fetchSmartMailboxes,
  });

  const effectiveEditorTarget =
    editorTarget !== "new" && !accounts.some((account) => account.id === editorTarget)
      ? (accounts[0]?.id ?? "new")
      : editorTarget;
  const editorAccountId = effectiveEditorTarget === "new" ? null : effectiveEditorTarget;
  const accountQuery = useQuery({
    queryKey: ["account", editorAccountId],
    queryFn: () => fetchAccount(editorAccountId!),
    enabled: editorAccountId !== null,
  });
  const editingAccount =
    accountQuery.data ??
    accounts.find((account) => account.id === editorAccountId) ??
    null;

  const smartMailboxSummaries = smartMailboxListQuery.data ?? [];
  const effectiveSmartMailboxTarget =
    smartMailboxEditorTarget !== "new" &&
    !smartMailboxSummaries.some((smartMailbox) => smartMailbox.id === smartMailboxEditorTarget)
      ? "new"
      : smartMailboxEditorTarget;
  const editingSmartMailboxId =
    effectiveSmartMailboxTarget === "new" ? null : effectiveSmartMailboxTarget;
  const smartMailboxQuery = useQuery({
    queryKey: ["smart-mailbox", editingSmartMailboxId],
    queryFn: () => fetchSmartMailbox(editingSmartMailboxId!),
    enabled: editingSmartMailboxId !== null,
  });
  const editingSmartMailbox =
    smartMailboxQuery.data ??
    smartMailboxSummaries.find((smartMailbox) => smartMailbox.id === editingSmartMailboxId) ??
    null;

  const enabledAccounts = useMemo(
    () => accounts.filter((account) => account.enabled),
    [accounts],
  );
  const accountSummary = useMemo(() => {
    const readyCount = accounts.filter((account) => account.status === "ready").length;
    return {
      total: accounts.length,
      readyCount,
      degradedCount: accounts.length - readyCount,
      enabledCount: enabledAccounts.length,
    };
  }, [accounts, enabledAccounts.length]);

  const invalidateAccountQueries = async (accountId?: string) => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ["settings"] }),
      queryClient.invalidateQueries({ queryKey: ["accounts"] }),
      accountId
        ? queryClient.invalidateQueries({ queryKey: ["account", accountId] })
        : Promise.resolve(),
    ]);
  };

  const invalidateSmartMailboxQueries = async (smartMailboxId?: string) => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ["sidebar"] }),
      queryClient.invalidateQueries({ queryKey: ["messages"] }),
      queryClient.invalidateQueries({ queryKey: ["smart-mailboxes"] }),
      smartMailboxId
        ? queryClient.invalidateQueries({ queryKey: ["smart-mailbox", smartMailboxId] })
        : Promise.resolve(),
    ]);
  };

  const runSmartMailboxAction = async (
    pendingKey: string,
    action: () => Promise<void>,
  ) => {
    if (smartMailboxActionPendingKey !== null) {
      return;
    }
    setSmartMailboxActionError(null);
    setSmartMailboxActionPendingKey(pendingKey);
    try {
      await action();
    } catch (error) {
      setSmartMailboxActionError(
        error instanceof Error ? error.message : "Smart mailbox action failed.",
      );
    } finally {
      setSmartMailboxActionPendingKey(null);
    }
  };

  const handleResetSmartMailboxes = () => {
    void runSmartMailboxAction("reset-defaults", async () => {
      await resetDefaultSmartMailboxes();
      await invalidateSmartMailboxQueries();
      setSmartMailboxEditorTarget("new");
    });
  };

  const handleReorderSmartMailbox = (
    smartMailbox: SmartMailboxSummary,
    position: number,
  ) => {
    void runSmartMailboxAction(`reorder:${smartMailbox.id}`, async () => {
      await updateSmartMailbox(smartMailbox.id, { position });
      await invalidateSmartMailboxQueries(smartMailbox.id);
    });
  };

  const defaultMutation = useMutation({
    mutationFn: (accountId: string | null) =>
      patchSettings({ defaultAccountId: accountId }),
    onSuccess: async () => {
      await invalidateAccountQueries();
    },
  });

  const commandMutation = useMutation({
    mutationFn: async ({
      action,
      account,
    }: {
      action: "enable" | "disable" | "delete" | "sync";
      account: AccountOverview;
    }) => {
      switch (action) {
        case "enable":
          return enableAccount(account.id);
        case "disable":
          return disableAccount(account.id);
        case "delete":
          return deleteAccount(account.id);
        case "sync":
          return triggerSync(account.id);
      }
    },
    onSuccess: async (_result, variables) => {
      await invalidateAccountQueries(variables.account.id);
      if (variables.action === "delete") {
        const fallbackAccountId =
          accounts.find(
            (account) =>
              account.id !== variables.account.id &&
              account.enabled &&
              account.isDefault,
          )?.id ??
          accounts.find(
            (account) => account.id !== variables.account.id && account.enabled,
          )?.id ??
          null;
        if (activeAccountId === variables.account.id) {
          onActiveAccountChange(fallbackAccountId);
        }
        if (effectiveEditorTarget === variables.account.id) {
          setEditorTarget(fallbackAccountId ?? "new");
        }
      }
    },
  });

  const editorKey =
    effectiveEditorTarget === "new"
      ? "new"
      : `${effectiveEditorTarget}:${editingAccount?.updatedAt ?? "pending"}`;
  const smartMailboxEditorKey =
    effectiveSmartMailboxTarget === "new"
      ? "new"
      : `${effectiveSmartMailboxTarget}:${editingSmartMailbox?.updatedAt ?? "pending"}`;

  return (
    <section className="flex h-full min-h-0 flex-col bg-card text-card-foreground">
      <div className="border-b border-border px-4 py-3">
        <p className="text-[10px] font-mono uppercase tracking-[0.24em] text-muted-foreground">
          posthaste
        </p>
        <div className="mt-2">
          <h2 className="text-lg font-semibold tracking-tight">Settings</h2>
          <p className="text-sm text-muted-foreground">
            Manage your mail accounts and preferences.
          </p>
        </div>
      </div>

      <div className="grid min-h-0 flex-1 grid-cols-[minmax(280px,0.85fr)_minmax(0,1.15fr)]">
        <div className="min-h-0 overflow-y-auto border-r border-border">
          <AccountListPane
            accounts={accounts}
            selectedAccountId={effectiveEditorTarget}
            defaultAccountId={settingsQuery.data?.defaultAccountId}
            accountSummary={accountSummary}
            onDefaultAccountChange={(accountId) => defaultMutation.mutate(accountId)}
            onCreateAccount={() => setEditorTarget("new")}
            onSelectAccount={(accountId) => setEditorTarget(accountId)}
            onCommand={(action, account) => commandMutation.mutate({ action, account })}
            defaultMutation={defaultMutation}
          />

          <SmartMailboxListPane
            smartMailboxSummaries={smartMailboxSummaries}
            selectedSmartMailboxId={effectiveSmartMailboxTarget}
            smartMailboxActionPendingKey={smartMailboxActionPendingKey}
            smartMailboxActionError={smartMailboxActionError}
            onResetDefaults={handleResetSmartMailboxes}
            onCreateMailbox={() => setSmartMailboxEditorTarget("new")}
            onSelectMailbox={(smartMailboxId) => setSmartMailboxEditorTarget(smartMailboxId)}
            onReorderMailbox={handleReorderSmartMailbox}
          />
        </div>

        <div className="min-h-0 space-y-4 overflow-y-auto px-4 py-4">
          <AccountEditor
            key={editorKey}
            editorTarget={effectiveEditorTarget}
            editingAccount={editingAccount}
            onSaved={async (account) => {
              await invalidateAccountQueries(account.id);
              setEditorTarget(account.id);
            }}
            onVerified={() => invalidateAccountQueries(editorAccountId ?? undefined)}
          />
          <SmartMailboxEditor
            key={smartMailboxEditorKey}
            editorTarget={effectiveSmartMailboxTarget}
            editingSmartMailbox={editingSmartMailbox}
            onSaved={async (smartMailbox) => {
              await invalidateSmartMailboxQueries(smartMailbox.id);
              setSmartMailboxEditorTarget(smartMailbox.id);
            }}
            onDeleted={async (smartMailboxId) => {
              await deleteSmartMailbox(smartMailboxId);
              await invalidateSmartMailboxQueries();
              setSmartMailboxEditorTarget("new");
            }}
          />
        </div>
      </div>
    </section>
  );
}
